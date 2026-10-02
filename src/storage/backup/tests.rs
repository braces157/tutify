use super::*;
use crate::{
    mix::{MixRecipe, MixSettings, MixSource},
    model::Repeat,
};
use serde_json::{Value, json};

fn store(parent: &Path, name: &str) -> Storage {
    let root = parent.join(name);
    fs::create_dir(&root).unwrap();
    Storage { root }
}

fn populate(store: &Storage) {
    store
        .save_config(&Config {
            volume: 71,
            repeat: Repeat::Queue,
            theme: "glass".into(),
            background_image: Some("wallpaper.jpg".into()),
            ..Config::default()
        })
        .unwrap();
    let mut queue = Queue::default();
    queue.replace(
        vec![
            format!("{:022}", 1),
            format!("{:022}", 2),
            format!("{:022}", 1),
            format!("{:022}", 3),
        ],
        0,
        false,
    );
    queue.order = vec![3, 0, 2, 1];
    queue.cursor = Some(2);
    queue.selected = 2;
    queue.position_ms = 45_678;
    queue.smart_shuffle = true;
    queue.suggestions.insert(2);
    store.save_queue(&queue).unwrap();
    let mut recipes = MixRecipes::default();
    recipes.save(MixRecipe {
        name: "Commute".into(),
        source: MixSource::Playlist {
            id: format!("{:022}", 11),
            name: "Saved 日本語".into(),
        },
        settings: MixSettings {
            target_minutes: 90,
            recommendation_percent: 37,
            artist_gap: 4,
        },
    });
    store.save_mix_recipes(&recipes).unwrap();
    let mut stats = SongStats::default();
    stats.add_play(&format!("{:022}", 1), "Song", "Artist");
    stats.add_play(&format!("{:022}", 1), "Song", "Artist");
    stats
        .tracks
        .get_mut(&format!("{:022}", 1))
        .unwrap()
        .listened_ms = 12_345;
    store.save_stats(&stats).unwrap();
}

fn originals(store: &Storage) -> Vec<Option<Vec<u8>>> {
    FILES
        .iter()
        .map(|name| read_bytes(&store.root.join(name), MAX_BYTES).unwrap())
        .collect()
}

fn apply(preview: RestorePreview, store: &Storage) {
    let token = preview.token.clone();
    preview.apply(store, &token).unwrap();
}

#[test]
fn backup_restore_roundtrip_preserves_duplicate_occurrences_settings_recipes_and_aggregates() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    let target = store(dir.path(), "target");
    populate(&source);
    let backup = dir.path().join("backup.json");
    source.backup(&backup).unwrap();
    let preview = target.restore_preview(&backup).unwrap();
    assert!(
        preview
            .files
            .iter()
            .all(|file| file.change == Change::Create)
    );
    assert!(preview.to_string().contains("4 queue occurrences"));
    assert!(originals(&target).iter().all(Option::is_none));
    apply(preview, &target);
    assert_eq!(target.config().unwrap(), source.config().unwrap());
    let queue = target.queue().unwrap();
    let original = source.queue().unwrap();
    assert_eq!(queue.ids, original.ids);
    assert_eq!(queue.ids[0], queue.ids[2]);
    assert_eq!(queue.order, vec![3, 0, 2, 1]);
    assert_eq!(queue.cursor, Some(2));
    assert_eq!(queue.selected, 2);
    assert_eq!(queue.position_ms, 45_678);
    assert!(queue.smart_shuffle);
    assert_eq!(queue.suggestions, original.suggestions);
    assert_eq!(
        target.mix_recipes().unwrap().recipes,
        source.mix_recipes().unwrap().recipes
    );
    assert_eq!(
        target.stats().unwrap().tracks,
        source.stats().unwrap().tracks
    );
    assert!(!target.root.join(JOURNAL).exists());
    let before = originals(&target);
    let preview = target.restore_preview(&backup).unwrap();
    assert!(
        preview
            .files
            .iter()
            .all(|file| file.change == Change::Unchanged)
    );
    apply(preview, &target);
    assert_eq!(originals(&target), before);
}

#[test]
fn backup_excludes_credentials_cache_runtime_fields_and_unknown_config_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    let target = store(dir.path(), "target");
    populate(&source);
    let mut config: Value =
        serde_json::from_slice(&fs::read(source.root.join("config.json")).unwrap()).unwrap();
    config["access_token"] = json!("CONFIG_SECRET_MARKER");
    config["native_glass"] = json!(true);
    fs::write(
        source.root.join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    for store in [&source, &target] {
        fs::write(store.root.join("tokens.json"), "CREDENTIAL_SECRET_MARKER").unwrap();
        fs::write(store.root.join("cache.json"), "CACHE_SECRET_MARKER").unwrap();
        fs::write(store.root.join("unrelated.txt"), "unrelated").unwrap();
    }
    let backup = dir.path().join("backup.json");
    source.backup(&backup).unwrap();
    let text = fs::read_to_string(&backup).unwrap();
    assert!(!text.contains("SECRET_MARKER"));
    assert!(!text.contains("access_token") && !text.contains("native_glass"));
    assert!(!text.contains("cache") && !text.contains("revision") && !text.contains("epoch"));
    apply(target.restore_preview(&backup).unwrap(), &target);
    assert_eq!(
        fs::read_to_string(target.root.join("tokens.json")).unwrap(),
        "CREDENTIAL_SECRET_MARKER"
    );
    assert_eq!(
        fs::read_to_string(target.root.join("cache.json")).unwrap(),
        "CACHE_SECRET_MARKER"
    );
    assert_eq!(
        fs::read_to_string(target.root.join("unrelated.txt")).unwrap(),
        "unrelated"
    );
}

#[test]
fn malformed_unsupported_and_invalid_backups_leave_all_current_files_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    let target = store(dir.path(), "target");
    populate(&source);
    populate(&target);
    let backup = dir.path().join("backup.json");
    source.backup(&backup).unwrap();
    let valid: Value = serde_json::from_slice(&fs::read(&backup).unwrap()).unwrap();
    let mut cases = vec![];
    let mut value = valid.clone();
    value["version"] = json!(99);
    cases.push(value);
    let mut value = valid.clone();
    value.as_object_mut().unwrap().remove("queue");
    cases.push(value);
    let mut value = valid.clone();
    value["queue"] = json!({"value":{}});
    cases.push(value);
    let mut value = valid.clone();
    value["credentials"] = json!("secret");
    cases.push(value);
    let mut value = valid.clone();
    value["format"] = json!("another-app");
    cases.push(value);
    let mut value = valid.clone();
    value["config"]["value"]["version"] = json!(2);
    cases.push(value);
    let mut value = valid.clone();
    value["config"]["value"]["volume"] = json!(101);
    cases.push(value);
    let mut value = valid.clone();
    value["queue"]["value"]["order"] = json!([0, 0, 2, 3]);
    cases.push(value);
    let mut value = valid.clone();
    value["mix_recipes"]["value"]["recipes"][0]["settings"]["recommendation_percent"] = json!(101);
    cases.push(value);
    let mut value = valid.clone();
    value["stats"]["value"]["version"] = json!(2);
    cases.push(value);
    let before = originals(&target);
    for (index, value) in cases.into_iter().enumerate() {
        fs::write(&backup, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(target.restore_preview(&backup).is_err(), "case {index}");
        assert_eq!(originals(&target), before, "case {index}");
        assert!(!target.root.join(JOURNAL).exists());
    }
    fs::write(&backup, b"{broken").unwrap();
    assert!(target.restore_preview(&backup).is_err());
    assert_eq!(originals(&target), before);
}

#[test]
fn previews_bind_confirmation_to_backup_current_state_and_destination() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    let target = store(dir.path(), "target");
    let other = store(dir.path(), "other");
    populate(&source);
    let backup = dir.path().join("backup.json");
    source.backup(&backup).unwrap();
    let preview = target.restore_preview(&backup).unwrap();
    let token = preview.token.clone();
    assert_ne!(other.restore_preview(&backup).unwrap().token, token);
    assert!(preview.apply(&target, &"0".repeat(64)).is_err());
    assert!(originals(&target).iter().all(Option::is_none));
    let preview = target.restore_preview(&backup).unwrap();
    target.save_config(&Config::default()).unwrap();
    let before = originals(&target);
    assert!(preview.apply(&target, &token).is_err());
    assert_eq!(originals(&target), before);
    assert_ne!(target.restore_preview(&backup).unwrap().token, token);
    let preview = target.restore_preview(&backup).unwrap();
    let token = preview.token.clone();
    let mut document: Value = serde_json::from_slice(&fs::read(&backup).unwrap()).unwrap();
    document["created_unix"] = json!(1);
    fs::write(&backup, serde_json::to_vec(&document).unwrap()).unwrap();
    assert!(preview.apply(&target, &token).is_err());
    assert_eq!(originals(&target), before);
    assert_ne!(target.restore_preview(&backup).unwrap().token, token);
    let preview = target.restore_preview(&backup).unwrap();
    let token = preview.token.clone();
    assert!(preview.apply(&other, &token).is_err());
    assert!(originals(&other).iter().all(Option::is_none));
}

#[test]
fn explicitly_missing_snapshots_remove_only_the_four_known_saved_state_files() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    let target = store(dir.path(), "target");
    populate(&target);
    fs::write(target.root.join("cache.json"), "cache").unwrap();
    let backup = dir.path().join("backup.json");
    source.backup(&backup).unwrap();
    let preview = target.restore_preview(&backup).unwrap();
    assert!(
        preview
            .files
            .iter()
            .all(|file| file.change == Change::Remove)
    );
    assert!(preview.to_string().contains("Remove"));
    apply(preview, &target);
    assert!(originals(&target).iter().all(Option::is_none));
    assert_eq!(
        fs::read_to_string(target.root.join("cache.json")).unwrap(),
        "cache"
    );
    assert!(target.queue().unwrap().ids.is_empty());
}

#[test]
fn backups_never_overwrite_existing_files_or_live_state_and_invalid_sources_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    populate(&source);
    let before = originals(&source);
    let destination = dir.path().join("existing.json");
    fs::write(&destination, "preserve me").unwrap();
    assert!(source.backup(&destination).is_err());
    assert_eq!(fs::read_to_string(&destination).unwrap(), "preserve me");
    assert!(source.backup(&source.root.join("queue.json")).is_err());
    assert!(source.backup(&source.root.join("new-backup.json")).is_err());
    assert_eq!(originals(&source), before);
    fs::write(source.root.join("queue.json"), "malformed").unwrap();
    let destination = dir.path().join("invalid.json");
    assert!(source.backup(&destination).is_err());
    assert!(!destination.exists());
    assert_eq!(
        fs::read_to_string(source.root.join("queue.json")).unwrap(),
        "malformed"
    );
}

#[test]
fn corrupt_existing_state_can_be_replaced_after_validation_and_explicit_preview_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    let target = store(dir.path(), "target");
    populate(&source);
    fs::write(target.root.join("queue.json"), "corrupt current state").unwrap();
    let backup = dir.path().join("backup.json");
    source.backup(&backup).unwrap();
    let preview = target.restore_preview(&backup).unwrap();
    assert_eq!(preview.files[1].change, Change::Replace);
    assert_eq!(
        fs::read_to_string(target.root.join("queue.json")).unwrap(),
        "corrupt current state"
    );
    apply(preview, &target);
    assert_eq!(target.queue().unwrap().ids, source.queue().unwrap().ids);
}

#[test]
fn oversized_files_and_directory_targets_are_rejected_before_any_changes() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    let target = store(dir.path(), "target");
    populate(&source);
    let backup = dir.path().join("backup.json");
    source.backup(&backup).unwrap();
    fs::create_dir(target.root.join("stats.json")).unwrap();
    assert!(target.restore_preview(&backup).is_err());
    assert!(!target.root.join("config.json").exists());
    assert!(target.root.join("stats.json").is_dir());
    let oversized = dir.path().join("oversized.json");
    fs::File::create(&oversized)
        .unwrap()
        .set_len(MAX_BYTES as u64 + 1)
        .unwrap();
    let error = target
        .restore_preview(&oversized)
        .err()
        .expect("oversized backup must fail");
    assert!(format!("{error:#}").contains("limit"));
}

fn journal_file(name: &str, before: Option<&[u8]>, after: Option<&[u8]>) -> JournalFile {
    JournalFile {
        name: name.into(),
        before_base64: before.map(|bytes| STANDARD.encode(bytes)),
        after_hash: after.map(hash),
    }
}

#[test]
fn next_locked_launch_recovers_an_interrupted_restore_with_creates_replaces_and_removals() {
    let dir = tempfile::tempdir().unwrap();
    let target = store(dir.path(), "target");
    let config_before = b"original config bytes";
    let config_after = b"replacement config bytes";
    let queue_after = b"new queue bytes";
    let stats_before = b"original statistics bytes";
    fs::write(target.root.join("config.json"), config_after).unwrap();
    fs::write(target.root.join("queue.json"), queue_after).unwrap();
    fs::write(target.root.join("cache.json"), "cache untouched").unwrap();
    atomic_json(
        &target.root.join(JOURNAL),
        &Journal {
            version: VERSION,
            committed: false,
            files: vec![
                journal_file(FILES[0], Some(config_before), Some(config_after)),
                journal_file(FILES[1], None, Some(queue_after)),
                journal_file(FILES[3], Some(stats_before), None),
            ],
        },
    )
    .unwrap();
    let _lock = target.lock().unwrap();
    assert_eq!(
        fs::read(target.root.join("config.json")).unwrap(),
        config_before
    );
    assert!(!target.root.join("queue.json").exists());
    assert_eq!(
        fs::read(target.root.join("stats.json")).unwrap(),
        stats_before
    );
    assert_eq!(
        fs::read_to_string(target.root.join("cache.json")).unwrap(),
        "cache untouched"
    );
    assert!(!target.root.join(JOURNAL).exists());
}

#[test]
fn every_interrupted_publish_prefix_recovers_exact_original_file_presence_and_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let before: [Option<&[u8]>; 4] = [
        Some(b"config before"),
        None,
        Some(b"recipe before"),
        Some(b"stats before"),
    ];
    let after: [Option<&[u8]>; 4] = [
        Some(b"config after"),
        Some(b"queue after"),
        None,
        Some(b"stats after"),
    ];
    for published in 0..=FILES.len() {
        let target = store(dir.path(), &format!("prefix-{published}"));
        for (index, name) in FILES.iter().enumerate() {
            if let Some(bytes) = if index < published {
                after[index]
            } else {
                before[index]
            } {
                fs::write(target.root.join(name), bytes).unwrap();
            }
        }
        let journal = Journal {
            version: VERSION,
            committed: false,
            files: FILES
                .iter()
                .enumerate()
                .map(|(index, name)| journal_file(name, before[index], after[index]))
                .collect(),
        };
        atomic_json(&target.root.join(JOURNAL), &journal).unwrap();
        let _lock = target.lock().unwrap();
        assert_eq!(
            originals(&target),
            before
                .iter()
                .map(|bytes| bytes.map(<[u8]>::to_vec))
                .collect::<Vec<_>>(),
            "published prefix {published}"
        );
        assert!(!target.root.join(JOURNAL).exists());
    }
}

#[test]
fn committed_restore_journal_only_cleans_up_and_never_reverts_newer_state() {
    let dir = tempfile::tempdir().unwrap();
    let target = store(dir.path(), "target");
    fs::write(target.root.join("config.json"), "newer user state").unwrap();
    atomic_json(
        &target.root.join(JOURNAL),
        &Journal {
            version: VERSION,
            committed: true,
            files: vec![journal_file(FILES[0], Some(b"before"), Some(b"after"))],
        },
    )
    .unwrap();
    let _lock = target.lock().unwrap();
    assert_eq!(
        fs::read_to_string(target.root.join("config.json")).unwrap(),
        "newer user state"
    );
    assert!(!target.root.join(JOURNAL).exists());
}

#[test]
fn malformed_journals_and_unexpected_manual_edits_preserve_journal_and_all_current_state() {
    let dir = tempfile::tempdir().unwrap();
    let target = store(dir.path(), "target");
    populate(&target);
    let before = originals(&target);
    for value in [
        json!({"version":99,"committed":false,"files":[]}),
        json!({"version":1,"committed":false,"files":[{"name":"../outside.json","before_base64":null,"after_hash":null}]}),
        json!({"version":1,"committed":false,"files":[{"name":"config.json","after_hash":null}]}),
        json!({"version":1,"committed":false,"files":[{"name":"config.json","before_base64":"not base64","after_hash":null}]}),
    ] {
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(target.root.join(JOURNAL), &bytes).unwrap();
        assert!(target.lock().is_err());
        assert_eq!(originals(&target), before);
        assert_eq!(fs::read(target.root.join(JOURNAL)).unwrap(), bytes);
    }
    atomic_json(
        &target.root.join(JOURNAL),
        &Journal {
            version: VERSION,
            committed: false,
            files: vec![journal_file(FILES[0], Some(b"before"), Some(b"after"))],
        },
    )
    .unwrap();
    assert!(target.lock().is_err());
    assert_eq!(originals(&target), before);
    assert!(target.root.join(JOURNAL).exists());
}

#[cfg(windows)]
#[test]
fn mid_restore_write_failure_rolls_back_published_files_and_preserves_readonly_original() {
    let dir = tempfile::tempdir().unwrap();
    let source = store(dir.path(), "source");
    let target = store(dir.path(), "target");
    populate(&source);
    target.save_config(&Config::default()).unwrap();
    target.save_queue(&Queue::default()).unwrap();
    target.save_mix_recipes(&MixRecipes::default()).unwrap();
    target.save_stats(&SongStats::default()).unwrap();
    let before = originals(&target);
    let backup = dir.path().join("backup.json");
    source.backup(&backup).unwrap();
    let preview = target.restore_preview(&backup).unwrap();
    let token = preview.token.clone();
    let path = target.root.join("mix-recipes.json");
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let result = preview.apply(&target, &token);
    fs::set_permissions(&path, original_permissions).unwrap();
    assert!(result.is_err());
    assert!(
        format!("{:#}", result.unwrap_err()).contains("original saved-state files were recovered")
    );
    assert_eq!(originals(&target), before);
    assert!(!target.root.join(JOURNAL).exists());
}
