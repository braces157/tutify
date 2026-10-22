use super::*;
use crate::deezer::tests::item;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

#[tokio::test]
async fn primary_search_needs_no_spotify_requests_and_fallback_paging_stays_native() {
    let native = MockServer::start().await;
    let primary = MockServer::start().await;
    Mock::given(path("/search"))
        .and(query_param("q", "Song"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[item(1,"Song")]})))
        .expect(1)
        .mount(&primary)
        .await;
    let catalog =
        Catalog::mock(&native.uri()).with_deezer(crate::deezer::Client::mock(&primary.uri()));
    let page = catalog
        .page(&Browse::Search("Song".into()), 0)
        .await
        .unwrap();
    assert!(matches!(page.rows, Rows::Tracks(tracks) if tracks[0].id=="deezer:1"));
    assert!(native.received_requests().await.unwrap().is_empty());
    Mock::given(path("/search"))
        .and(query_param("q", "fallback"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .expect(1)
        .mount(&primary)
        .await;
    Mock::given(path("/search"))
        .and(query_param("q", "fallback"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"tracks":{"items":[{
            "id":"0000000000000000000001","name":"Song","artists":[{"name":"Artist"}],"type":"track"
        }],"next":"next"}})),
        )
        .expect(2)
        .mount(&native)
        .await;
    let browse = Browse::Search("fallback".into());
    let first = catalog.page(&browse, 0).await.unwrap();
    let next = first.next.unwrap();
    assert!(
        matches!(catalog.page(&browse,next).await.unwrap().rows,Rows::Tracks(tracks) if tracks[0].id=="0000000000000000000001")
    );
}

#[tokio::test]
async fn playback_uses_spotify_isrc_and_rejects_wrong_versions_in_metadata_fallback() {
    for exact in [true, false] {
        let native = MockServer::start().await;
        let primary = MockServer::start().await;
        let mut song = item(1, "Song");
        if exact {
            song["isrc"] = json!("USABC2400001");
        }
        Mock::given(path("/track/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(song))
            .mount(&primary)
            .await;
        Mock::given(path("/search")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"tracks":{"items":[
            {"id":"0000000000000000000001","name":"Song (Live)","artists":[{"name":"Artist"}],"duration_ms":200000,"external_ids":{"isrc":"USABC2400002"}},
            {"id":"0000000000000000000002","name":"Song","artists":[{"name":"Cover Artist"}],"duration_ms":200000},
            {"id":"0000000000000000000003","name":"Song","artists":[{"name":"Artist"}],"duration_ms":200000,"external_ids":{"isrc":"USABC2400001"}}
        ]}}))).expect(1).mount(&native).await;
        let catalog =
            Catalog::mock(&native.uri()).with_deezer(crate::deezer::Client::mock(&primary.uri()));
        let (track, id) = catalog.resolve_playback_track("deezer:1").await.unwrap();
        assert_eq!(track.id, "deezer:1");
        assert_eq!(id, "0000000000000000000003");
    }
}

#[tokio::test]
async fn volta_resolves_deezer_isrc_without_a_spotify_lookup() {
    let native = MockServer::start().await;
    let primary = MockServer::start().await;
    let mut song = item(1, "Song");
    song["readable"] = json!(false);
    song["isrc"] = json!("USABC2400001");
    Mock::given(path("/track/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(song))
        .mount(&primary)
        .await;
    Mock::given(path("/oauth/token")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token":"volta_at_test","expires_in":3600,"scope":"catalog:read stream:read"}))).mount(&native).await;
    Mock::given(path("/track/USABC2400001")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"USABC2400001","name":"Translated song","artist":"Translated artist","duration_ms":200000}))).expect(1).mount(&native).await;
    let catalog = Catalog::volta(crate::volta::Client::mock(&native.uri()))
        .unwrap()
        .with_deezer(crate::deezer::Client::mock(&primary.uri()));
    let (track, id) = catalog.resolve_playback_track("deezer:1").await.unwrap();
    assert_eq!(track.id, "deezer:1");
    assert!(track.playable);
    assert_eq!(id, "volta:USABC2400001");
}

#[tokio::test]
async fn youtube_resolves_official_music_and_keeps_deezer_identity() {
    let primary = MockServer::start().await;
    Mock::given(path("/track/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(item(1, "Song")))
        .mount(&primary)
        .await;
    let music = crate::youtube::music::Client::mock(
        r#"import json,sys
r=json.load(sys.stdin)
assert r['operation']=='search'
json.dump({'ok':True,'complete':True,'items':[
{'videoId':'aaaaaaaaaaa','title':'Song (Live)','artists':[{'name':'Artist'}],'duration':'3:20'},
{'videoId':'bbbbbbbbbbb','title':'Song','artists':[{'name':'Cover Artist'}],'duration':'3:20'},
{'videoId':'ccccccccccc','title':'Song','artists':[{'name':'Artist'}],'duration':'3:20'}]},sys.stdout)
"#,
        false,
    );
    let catalog = Catalog::youtube(crate::youtube::Tools::music_fixture(music))
        .unwrap()
        .with_deezer(crate::deezer::Client::mock(&primary.uri()));
    let (track, id) = catalog.resolve_playback_track("deezer:1").await.unwrap();
    assert_eq!(track.id, "deezer:1");
    assert_eq!(id, "youtube:ccccccccccc");
}

#[tokio::test]
#[ignore = "Read-only live Deezer-to-Volta recording matching; uses saved personal credentials, no audio"]
async fn live_deezer_to_volta_recordings() -> Result<()> {
    let deezer = crate::deezer::Client::new()?;
    let catalog = Catalog::volta(crate::volta::Client::load()?)?.with_deezer(deezer.clone());
    for query in [
        "Radiohead Creep",
        "Mưa Tháng Sáu Văn Mai Hương",
        "ヨルシカ だから僕は音楽を辞めた",
    ] {
        let page = deezer.page(&Browse::Search(query.into()), 0).await?;
        let Rows::Tracks(tracks) = page.rows else {
            unreachable!()
        };
        let track = tracks.first().context("No Deezer results")?;
        let (metadata, id) = catalog.resolve_playback_track(&track.id).await?;
        println!("{} / {} -> {id}", metadata.name, metadata.artists);
    }
    Ok(())
}
