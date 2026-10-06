use super::*;

fn client(script: &str, connected: bool) -> Client {
    Client::mock(script, connected)
}

#[tokio::test]
async fn expired_optional_library_login_does_not_block_public_music_search() {
    let client = client(
        r#"import json,sys
r=json.load(sys.stdin)
if r['operation']=='search':
    assert 'headers' not in r
    json.dump({'ok':True,'items':[{'videoId':'dQw4w9WgXcQ','title':'Public song'}],'complete':True},sys.stdout)
else:
    assert 'headers' in r
    json.dump({'ok':False,'error':'authentication'},sys.stdout)
"#,
        true,
    );
    assert!(
        matches!(client.page(&Browse::Search("Public song".into()),0).await.unwrap().rows,Rows::Tracks(rows) if rows.len()==1)
    );
    let error = client.page(&Browse::Liked, 0).await.unwrap_err();
    assert!(ServiceFailure::is(
        &error,
        FailureKind::AuthenticationRequired
    ));
    assert!(
        matches!(client.page(&Browse::Search("Other song".into()),0).await.unwrap().rows,Rows::Tracks(rows) if rows.len()==1)
    );
}

#[tokio::test]
async fn idle_helper_is_released_without_losing_cached_pages_and_restarts_on_demand() {
    let client = client(
        r#"import json,os,sys
r=json.load(sys.stdin)
json.dump({'ok':True,'items':[{'videoId':'dQw4w9WgXcQ','title':'Song'}],'complete':True,'pid':os.getpid()},sys.stdout)
"#,
        true,
    );
    client.page(&Browse::Liked, 0).await.unwrap();
    let first = client
        .rpc(json!({"operation":"test"}), client.auth())
        .await
        .unwrap();
    client.worker.lock().await.as_mut().unwrap().last_used -= Duration::from_secs(31);
    client.release_idle_helper();
    assert!(client.worker.lock().await.is_none());
    let cached = client.page(&Browse::Liked, 0).await.unwrap();
    assert!(matches!(cached.rows,Rows::Tracks(rows) if rows.len()==1));
    assert!(client.worker.lock().await.is_none());
    let second = client
        .rpc(json!({"operation":"test"}), client.auth())
        .await
        .unwrap();
    assert_ne!(first["pid"], second["pid"]);
}

#[tokio::test]
async fn helper_is_reused_and_cancelled_requests_kill_it_without_blocking_cached_pages() {
    let client = client(
        r#"import json,os,sys,time
r=json.load(sys.stdin)
if r['operation']=='slow':
    time.sleep(60)
json.dump({'ok':True,'items':[{'videoId':'dQw4w9WgXcQ','title':'Song'}],'complete':True,'pid':os.getpid()},sys.stdout)
"#,
        true,
    );
    let one = client
        .rpc(json!({"operation":"test"}), client.auth())
        .await
        .unwrap();
    let two = client
        .clone()
        .rpc(json!({"operation":"test"}), client.auth())
        .await
        .unwrap();
    assert_eq!(one["pid"], two["pid"]);
    client.page(&Browse::Liked, 0).await.unwrap();
    let slow_client = client.clone();
    let task = tokio::spawn(async move {
        slow_client
            .rpc(json!({"operation":"slow"}), slow_client.auth())
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while client.worker.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let cached = tokio::time::timeout(Duration::from_millis(250), client.page(&Browse::Liked, 0))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(cached.rows, Rows::Tracks(tracks) if tracks.len() == 1));
    task.abort();
    let _ = task.await;
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    unsafe {
        if let Ok(handle) = OpenProcess(
            PROCESS_SYNCHRONIZE,
            false,
            one["pid"].as_u64().unwrap() as u32,
        ) {
            assert_eq!(WaitForSingleObject(handle, 5000), WAIT_OBJECT_0);
            CloseHandle(handle).unwrap();
        }
    }
    let next = client
        .rpc(json!({"operation":"test"}), client.auth())
        .await
        .unwrap();
    assert_ne!(one["pid"], next["pid"]);
}

#[tokio::test]
async fn first_search_is_one_batch_and_pagination_keeps_all_results() {
    let client = client(
        r#"import json,sys
r=json.load(sys.stdin)
limit=r['limit']
items=[{'videoId':'v%010d'%i,'title':'Song '+str(i)} for i in range(min(limit,43))]
json.dump({'ok':True,'items':items,'complete':len(items)<limit},sys.stdout)
"#,
        false,
    );
    let browse = Browse::Search("Artist song".into());
    let one = client.page(&browse, 0).await.unwrap();
    assert!(matches!(one.rows, Rows::Tracks(rows) if rows.len()==20));
    assert_eq!(
        client
            .cache
            .lock()
            .await
            .get("search:Artist song", 0)
            .unwrap()
            .limit,
        20
    );
    assert_eq!(one.next, Some(20));
    let two = client.page(&browse, 20).await.unwrap();
    assert!(matches!(two.rows, Rows::Tracks(rows) if rows.len()==20 && rows[0].name=="Song 20"));
    let three = client.page(&browse, 40).await.unwrap();
    assert!(matches!(three.rows, Rows::Tracks(rows) if rows.len()==3));
    assert!(three.next.is_none());
}

#[tokio::test]
#[ignore = "Read-only latency check using the locally connected Google account and live network"]
async fn live_music_request_latency() {
    let shared = Client::discover().unwrap().unwrap();
    for query in [
        "Laufey From The Start",
        "Laufey Promise",
        "Laufey Bewitched",
    ] {
        let begin = Instant::now();
        let page = shared.page(&Browse::Search(query.into()), 0).await.unwrap();
        let Rows::Tracks(rows) = page.rows else {
            panic!("expected music results")
        };
        println!(
            "Music search: {:.3}s, {} rows",
            begin.elapsed().as_secs_f64(),
            rows.len()
        );
    }
    for browse in [Browse::Playlists, Browse::Liked] {
        let begin = Instant::now();
        shared.page(&browse, 0).await.unwrap();
        println!(
            "Music library request: {:.3}s",
            begin.elapsed().as_secs_f64()
        );
        let begin = Instant::now();
        shared.page(&browse, 0).await.unwrap();
        println!(
            "Cached library request: {:.3}s",
            begin.elapsed().as_secs_f64()
        );
    }
}

#[test]
fn bridge_routes_readonly_operations_and_redacts_sensitive_payloads() {
    let script = format!(
        "namespace={{'__name__':'bridge_test'}}\nexec({},namespace)\n{}",
        serde_json::to_string(include_str!("../music_bridge.py")).unwrap(),
        include_str!("bridge_tests.py")
    );
    let client = client("", false);
    let result = std::process::Command::new(client.python)
        .args(["-I", "-c", &script])
        .output()
        .expect("Python 3.10+ is required for bridge contract tests");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn music_ids_and_metadata_are_provider_scoped() {
    assert_eq!(
        playlist_id("https://music.youtube.com/playlist?list=PL_example123").as_deref(),
        Some("youtube:playlist:PL_example123")
    );
    assert!(
        playlist_id("https://music.youtube.com.evil.test/playlist?list=PL_example123").is_none()
    );
    assert!(playlist_id("https://evil.test/playlist?list=PL_example123").is_none());
    assert!(crate::model::valid_playlist_id(
        "youtube:playlist:PL_example123"
    ));
    assert!(!crate::model::valid_track_id(
        "youtube:playlist:PL_example123"
    ));
    let track = parse_track(&json!({"videoId":"dQw4w9WgXcQ", "title":"Song", "artists":[{"name":"Artist","id":"UC_artist123"}], "album":{"name":"Album", "id":"MPRE_album123"}, "duration":"3:33", "isAvailable":false})).unwrap();
    assert_eq!(track.duration_ms, 213000);
    assert!(track.music_metadata);
    assert!(!track.playable);
    assert_eq!(track.artist_ids, vec!["youtube:artist:UC_artist123"]);
    assert_eq!(
        track.album_id.as_deref(),
        Some("youtube:album:MPRE_album123")
    );
}

#[tokio::test]
async fn playlist_paging_has_no_truncation_and_cached_pages_need_no_rpc() {
    let script = r#"import json,sys
r=json.load(sys.stdin)
n=min(r['limit'],123)
items=[{'videoId':'v%010d'%i,'title':'Song '+str(i),'artists':[{'name':'Artist','id':'UC_artist'}],'duration_seconds':180} for i in range(n)]
json.dump({'ok':True,'items':items,'complete':n==123},sys.stdout)
"#;
    let client = client(script, true);
    let browse = Browse::Playlist("youtube:playlist:PL_example123".into());
    let mut total = 0;
    let mut offset = 0;
    loop {
        let page = client.page(&browse, offset).await.unwrap();
        let Rows::Tracks(tracks) = page.rows else {
            panic!("wrong rows")
        };
        assert_eq!(tracks.first().unwrap().name, format!("Song {offset}"));
        total += tracks.len();
        match page.next {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(total, 123);
    let cached = client.page(&browse, 0).await.unwrap();
    assert!(matches!(cached.rows, Rows::Tracks(tracks) if tracks.len()==50));
    client.refresh();
    assert!(client.cache.lock().await.is_empty());
}

#[tokio::test]
async fn authentication_and_service_errors_do_not_expose_credentials() {
    let anonymous = client("raise Exception('must not be called')", false);
    let error = anonymous.page(&Browse::Liked, 0).await.unwrap_err();
    assert!(ServiceFailure::is(
        &error,
        FailureKind::AuthenticationRequired
    ));
    assert!(error.to_string().contains("Connect Google music library"));
    let connected = client(
        "import json,sys; json.load(sys.stdin); json.dump({'ok':False,'error':'restricted','secret':'fixture-private-cookie'},sys.stdout)",
        true,
    );
    let error = connected
        .page(&Browse::Liked, 0)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("denied access"));
    assert!(!error.contains("fixture-private-cookie"));
    let connected = client(
        "import sys;sys.stderr.write('fixture-private-token');sys.exit(1)",
        true,
    );
    let error = connected
        .page(&Browse::Liked, 0)
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("fixture-private-token"));
}
