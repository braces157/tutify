use super::*;
use crate::{
    mix::{MixRecipe, MixRecipes, MixSettings, MixSource},
    queue::Queue,
};
use serde_json::json;

fn store(parent: &Path, name: &str) -> Storage {
    let root = parent.join(name);
    fs::create_dir(&root).unwrap();
    Storage { root }
}
fn populated(parent: &Path, name: &str) -> Storage {
    let store = store(parent, name);
    store.save_config(&Config::default()).unwrap();
    let mut queue = Queue::default();
    queue.replace(
        vec!["0".repeat(22), "1".repeat(22), "0".repeat(22)],
        2,
        false,
    );
    queue.position_ms = 12_345;
    store.save_queue(&queue).unwrap();
    let mut recipes = MixRecipes::default();
    recipes.save(MixRecipe {
        name: "Commute 日本語".into(),
        source: MixSource::Queue,
        settings: MixSettings {
            target_minutes: 90,
            recommendation_percent: 37,
            artist_gap: 4,
        },
    });
    store.save_mix_recipes(&recipes).unwrap();
    store
        .save_stats(&crate::stats::SongStats::default())
        .unwrap();
    store
        .save_cache(&crate::cache::MetadataCache::default())
        .unwrap();
    fs::write(
        store.root.join("credential-fixture.txt"),
        b"must never be accessed or changed",
    )
    .unwrap();
    store
}
fn originals(store: &Storage) -> Vec<(String, Vec<u8>)> {
    let mut files = fs::read_dir(&store.root)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            entry.path().is_file().then(|| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}
fn unchanged_other_files(before: &[(String, Vec<u8>)], store: &Storage, selected: StateFile) {
    for (name, bytes) in before {
        if name != selected.name() {
            assert_eq!(
                fs::read(store.root.join(name)).unwrap(),
                *bytes,
                "{name} changed"
            );
        }
    }
}
fn apply(preview: RecoveryPreview, store: &Storage) {
    let token = preview.token.clone();
    preview.apply(store, &token).unwrap();
}

#[test]
fn inspection_reports_file_version_and_invariant_without_raw_values_or_writes() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path(), "live");
    for (file, bytes, kind, expected) in [
        (StateFile::Recipes, br#"{"version":99,"unknown":"private-secret"}"#.as_slice(), StateFailureKind::UnsupportedVersion(99), "Unsupported version 99"),
        (StateFile::Config, br#"{"version":1,"volume":"private-secret"}"#.as_slice(), StateFailureKind::Malformed, "Invalid field type"),
        (StateFile::Queue, br#"{"version":1,"ids":["0000000000000000000000"],"order":[2]}"#.as_slice(), StateFailureKind::Invariant, "invalid order"),
        (StateFile::Stats, br#"{"version":1,"tracks":{"private-secret":{"id":"0000000000000000000000","name":"Name","artists":"Artist","play_count":1,"listened_ms":1}}}"#.as_slice(), StateFailureKind::Invariant, "matching their entry ID"),
        (StateFile::Recipes, br#"{"version":1,"recipes":[{"name":"private-secret","source":"Queue","settings":{"target_minutes":0}}]}"#.as_slice(), StateFailureKind::Invariant, "Mix target"),
    ] {
        fs::write(store.root.join(file.name()), bytes).unwrap();
        let before = originals(&store);
        let inspection = store.inspect_state(file);
        let message = inspection.to_string();
        let error = inspection.result.unwrap_err();
        let error = error.downcast_ref::<StateFailure>().unwrap();
        assert_eq!(error.file, file); assert_eq!(error.kind, kind);
        assert!(message.contains(file.name())); assert!(message.contains(expected), "{message}");
        assert!(message.contains("state backup")); assert!(!message.contains("private-secret"));
        assert_eq!(originals(&store), before);
    }
}

#[test]
fn missing_files_inspect_as_defaults_and_reset_is_an_inert_confirmed_preview() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path(), "live");
    for file in StateFile::ALL {
        assert!(store.inspect_state(file).result.is_ok());
        assert!(
            store
                .backup_component(file, &dir.path().join("missing.json"))
                .is_err()
        );
        apply(store.recovery_preview(file, None, None).unwrap(), &store);
    }
    assert!(originals(&store).is_empty());
}

#[test]
fn damaged_recipes_reset_preserves_original_bytes_queue_position_and_other_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path(), "live");
    let damaged = b"{ damaged recipes with private-secret";
    fs::write(store.root.join("mix-recipes.json"), damaged).unwrap();
    let before = originals(&store);
    let preview = store
        .recovery_preview(StateFile::Recipes, None, None)
        .unwrap();
    let archive = preview.archive.clone().unwrap();
    assert!(!archive.exists());
    assert_eq!(originals(&store), before);
    assert!(
        store
            .recovery_preview(StateFile::Recipes, None, None)
            .unwrap()
            .apply(&store, &"0".repeat(64))
            .is_err()
    );
    assert!(!archive.exists());
    apply(preview, &store);
    assert!(!store.root.join("mix-recipes.json").exists());
    assert!(store.mix_recipes().unwrap().recipes.is_empty());
    assert_eq!(
        extract(&fs::read(archive).unwrap(), StateFile::Recipes)
            .unwrap()
            .unwrap(),
        damaged
    );
    unchanged_other_files(&before, &store, StateFile::Recipes);
    assert_eq!(store.queue().unwrap().cursor, Some(2));
    assert_eq!(store.queue().unwrap().position_ms, 12_345);
}

#[test]
fn component_restore_preserves_unknown_fields_and_changes_only_selected_raw_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let source = populated(dir.path(), "source");
    let target = populated(dir.path(), "target");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(source.root.join("mix-recipes.json")).unwrap()).unwrap();
    value["future_extra"] = json!({"keep":"extension bytes"});
    let raw = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(source.root.join("mix-recipes.json"), &raw).unwrap();
    let backup = dir.path().join("recipes.json");
    source
        .backup_component(StateFile::Recipes, &backup)
        .unwrap();
    fs::write(target.root.join("mix-recipes.json"), b"damaged target").unwrap();
    let before = originals(&target);
    apply(
        target
            .recovery_preview(StateFile::Recipes, Some(&backup), None)
            .unwrap(),
        &target,
    );
    assert_eq!(fs::read(target.root.join("mix-recipes.json")).unwrap(), raw);
    assert_eq!(
        target.mix_recipes().unwrap().recipes[0]
            .settings
            .target_minutes,
        90
    );
    unchanged_other_files(&before, &target, StateFile::Recipes);
}

#[test]
fn whole_backup_can_restore_one_file_despite_unrelated_damaged_current_state() {
    let dir = tempfile::tempdir().unwrap();
    let source = populated(dir.path(), "source");
    let target = populated(dir.path(), "target");
    let backup = dir.path().join("whole.json");
    source.backup(&backup).unwrap();
    fs::write(
        target.root.join("queue.json"),
        b"damaged queue must not block recipe recovery",
    )
    .unwrap();
    fs::write(target.root.join("stats.json"), br#"{"version":99}"#).unwrap();
    fs::write(target.root.join("mix-recipes.json"), b"damaged recipes").unwrap();
    let before = originals(&target);
    apply(
        target
            .recovery_preview(StateFile::Recipes, Some(&backup), None)
            .unwrap(),
        &target,
    );
    assert_eq!(
        target.mix_recipes().unwrap().recipes,
        source.mix_recipes().unwrap().recipes
    );
    unchanged_other_files(&before, &target, StateFile::Recipes);
    assert!(
        target
            .recovery_preview(StateFile::Cache, Some(&backup), None)
            .is_err()
    );
}

#[test]
fn future_state_versions_are_not_overwritten_by_normal_reads_or_writers() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path(), "live");
    for file in StateFile::ALL {
        fs::write(
            store.root.join(file.name()),
            br#"{"version":999,"new_format":{"private":"secret"}}"#,
        )
        .unwrap();
    }
    let before = originals(&store);
    assert!(store.config().is_err());
    assert!(store.queue().is_err());
    assert!(store.stats().is_err());
    assert!(store.mix_recipes().is_err());
    assert!(store.cache().is_err());
    assert!(store.save_config(&Config::default()).is_err());
    assert!(store.save_queue(&Queue::default()).is_err());
    assert!(
        store
            .save_stats(&crate::stats::SongStats::default())
            .is_err()
    );
    assert!(store.save_mix_recipes(&MixRecipes::default()).is_err());
    assert!(
        store
            .save_cache(&crate::cache::MetadataCache::default())
            .is_err()
    );
    assert_eq!(originals(&store), before);
    let source = populated(dir.path(), "supported-source");
    let whole = dir.path().join("whole.json");
    source.backup(&whole).unwrap();
    assert!(store.restore_preview(&whole).is_err());
    assert_eq!(originals(&store), before);
    let backup = dir.path().join("future.json");
    store.backup_component(StateFile::Recipes, &backup).unwrap();
    assert!(
        store
            .recovery_preview(StateFile::Recipes, Some(&backup), None)
            .is_err()
    );
    let preview = store
        .recovery_preview(StateFile::Recipes, None, None)
        .unwrap();
    let archive = preview.archive.clone().unwrap();
    apply(preview, &store);
    assert_eq!(
        extract(&fs::read(archive).unwrap(), StateFile::Recipes)
            .unwrap()
            .unwrap(),
        br#"{"version":999,"new_format":{"private":"secret"}}"#
    );
    unchanged_other_files(&before, &store, StateFile::Recipes);
}

#[test]
fn targeted_missing_snapshot_restores_defaults_and_preserves_future_original() {
    let dir = tempfile::tempdir().unwrap();
    let source = populated(dir.path(), "source");
    fs::remove_file(source.root.join("mix-recipes.json")).unwrap();
    let backup = dir.path().join("missing-recipes.json");
    source.backup(&backup).unwrap();
    let target = populated(dir.path(), "target");
    fs::write(
        target.root.join("mix-recipes.json"),
        br#"{"version":999,"future":"data"}"#,
    )
    .unwrap();
    let before = originals(&target);
    let preview = target
        .recovery_preview(StateFile::Recipes, Some(&backup), None)
        .unwrap();
    let archive = preview.archive.clone().unwrap();
    apply(preview, &target);
    assert!(!target.root.join("mix-recipes.json").exists());
    assert!(target.mix_recipes().unwrap().recipes.is_empty());
    assert_eq!(
        extract(&fs::read(archive).unwrap(), StateFile::Recipes)
            .unwrap()
            .unwrap(),
        br#"{"version":999,"future":"data"}"#
    );
    unchanged_other_files(&before, &target, StateFile::Recipes);
}

#[test]
fn corrupt_cache_is_disposable_without_modifying_queue_or_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path(), "live");
    fs::write(store.root.join("cache.json"), b"corrupt cache").unwrap();
    let before = originals(&store);
    assert!(store.cache().is_err());
    let preview = store
        .recovery_preview(StateFile::Cache, None, None)
        .unwrap();
    let archive = preview.archive.clone().unwrap();
    apply(preview, &store);
    assert!(store.cache().is_ok());
    assert!(!store.root.join("cache.json").exists());
    assert_eq!(
        extract(&fs::read(archive).unwrap(), StateFile::Cache)
            .unwrap()
            .unwrap(),
        b"corrupt cache"
    );
    unchanged_other_files(&before, &store, StateFile::Cache);
}

#[test]
fn backup_kind_version_integrity_and_selected_snapshot_validation_gate_restore() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path(), "live");
    let before = originals(&store);
    let backup = dir.path().join("backup.json");
    store.backup_component(StateFile::Recipes, &backup).unwrap();
    let valid = fs::read(&backup).unwrap();
    for modified in [
        json!({"format":FORMAT,"version":2,"component":"recipes","sha256":"bad","bytes_base64":""}),
        json!({"format":FORMAT,"version":1,"component":"queue","sha256":"bad","bytes_base64":""}),
        json!({"format":FORMAT,"version":1,"component":"recipes","sha256":"bad","bytes_base64":"AAAA"}),
        json!({"format":FORMAT,"version":1,"component":"recipes","sha256":"bad","bytes_base64":"%%%"}),
    ] {
        fs::write(&backup, serde_json::to_vec(&modified).unwrap()).unwrap();
        assert!(
            store
                .recovery_preview(StateFile::Recipes, Some(&backup), None)
                .is_err()
        );
        assert_eq!(originals(&store), before);
    }
    for raw in [br#"{"version":99}"#.as_slice(), br#"{"version":1,"recipes":[{"name":"bad","source":"Queue","settings":{"artist_gap":99}}]}"#.as_slice(), b"broken JSON"] {
        fs::write(&backup, envelope(StateFile::Recipes, raw).unwrap()).unwrap();
        assert!(store.recovery_preview(StateFile::Recipes, Some(&backup), None).is_err()); assert_eq!(originals(&store), before);
    }
    fs::write(&backup, valid).unwrap();
    assert!(
        store
            .recovery_preview(StateFile::Stats, Some(&backup), None)
            .is_err()
    );
}

#[test]
fn confirmation_binds_selected_bytes_destination_source_action_and_archive() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path(), "live");
    let backup = dir.path().join("recipes.json");
    store.backup_component(StateFile::Recipes, &backup).unwrap();
    let reset = store
        .recovery_preview(StateFile::Recipes, None, None)
        .unwrap();
    let reset_token = reset.token.clone();
    fs::write(
        store.root.join("mix-recipes.json"),
        b"changed after preview",
    )
    .unwrap();
    assert!(reset.apply(&store, &reset_token).is_err());
    let reset = store
        .recovery_preview(StateFile::Recipes, None, None)
        .unwrap();
    let restore = store
        .recovery_preview(StateFile::Recipes, Some(&backup), None)
        .unwrap();
    assert_ne!(reset.token, restore.token);
    assert!(restore.apply(&store, &reset.token).is_err());
    let restore = store
        .recovery_preview(StateFile::Recipes, Some(&backup), None)
        .unwrap();
    let token = restore.token.clone();
    let backup_original = fs::read(&backup).unwrap();
    fs::write(&backup, b"source changed").unwrap();
    assert!(restore.apply(&store, &token).is_err());
    fs::write(&backup, backup_original).unwrap();
    let other = populated(dir.path(), "other");
    let restore = store
        .recovery_preview(StateFile::Recipes, Some(&backup), None)
        .unwrap();
    let token = restore.token.clone();
    assert!(restore.apply(&other, &token).is_err());
    let normal = store
        .recovery_preview(StateFile::Recipes, None, None)
        .unwrap();
    let changed_archive = store
        .recovery_preview(
            StateFile::Recipes,
            None,
            Some(&dir.path().join("different-archive.json")),
        )
        .unwrap();
    assert_ne!(normal.token, changed_archive.token);
    // Changes to unrelated state neither invalidate this preview nor get reset.
    fs::write(
        store.root.join("queue.json"),
        b"unrelated change must survive",
    )
    .unwrap();
    apply(normal, &store);
    assert_eq!(
        fs::read(store.root.join("queue.json")).unwrap(),
        b"unrelated change must survive"
    );
}

#[test]
fn backup_exports_never_clobber_or_target_live_files_and_archive_collisions_stop_reset() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path(), "live");
    let before = originals(&store);
    let backup = dir.path().join("existing.json");
    fs::write(&backup, b"keep this existing artifact").unwrap();
    assert!(store.backup_component(StateFile::Recipes, &backup).is_err());
    assert_eq!(fs::read(&backup).unwrap(), b"keep this existing artifact");
    assert!(
        store
            .backup_component(StateFile::Recipes, &store.root.join("new.json"))
            .is_err()
    );
    let preview = store
        .recovery_preview(StateFile::Recipes, None, Some(&backup))
        .unwrap();
    let token = preview.token.clone();
    assert!(preview.apply(&store, &token).is_err());
    assert_eq!(originals(&store), before);
}

#[test]
fn read_only_restore_failure_recovers_originals_and_retains_the_component_backup() {
    for file in [StateFile::Recipes, StateFile::Cache] {
        let dir = tempfile::tempdir().unwrap();
        let store = populated(dir.path(), "live");
        let path = store.root.join(file.name());
        let permissions = fs::metadata(&path).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly).unwrap();
        let mut incoming: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        incoming["extension"] = json!("replacement bytes");
        let source = dir.path().join("incoming.json");
        fs::write(
            &source,
            envelope(file, &serde_json::to_vec(&incoming).unwrap()).unwrap(),
        )
        .unwrap();
        let before = originals(&store);
        let preview = store.recovery_preview(file, Some(&source), None).unwrap();
        let archive = preview.archive.clone().unwrap();
        let token = preview.token.clone();
        let result = preview.apply(&store, &token);
        let after = originals(&store);
        fs::set_permissions(&path, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(after, before);
        assert!(archive.exists());
        assert!(!store.root.join("restore-journal.json").exists());
    }
}

#[test]
fn oversized_and_non_regular_targets_are_preserved_and_not_backed_up_or_reset() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path(), "live");
    let path = store.root.join("cache.json");
    fs::create_dir(&path).unwrap();
    assert!(store.inspect_state(StateFile::Cache).result.is_err());
    assert!(
        store
            .recovery_preview(StateFile::Cache, None, None)
            .is_err()
    );
    fs::remove_dir(&path).unwrap();
    fs::File::create(&path)
        .unwrap()
        .set_len(MAX_BYTES as u64 + 1)
        .unwrap();
    assert!(store.inspect_state(StateFile::Cache).result.is_err());
    assert!(
        store
            .backup_component(StateFile::Cache, &dir.path().join("large.json"))
            .is_err()
    );
    assert_eq!(fs::metadata(&path).unwrap().len(), MAX_BYTES as u64 + 1);
}

#[test]
fn locked_startup_recovers_interrupted_cache_reset_using_the_shared_journal() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path(), "live");
    let original = fs::read(store.root.join("cache.json")).unwrap();
    fs::remove_file(store.root.join("cache.json")).unwrap();
    let journal = json!({"version":1,"committed":false,"files":[{"name":"cache.json","before_base64":STANDARD.encode(&original),"after_hash":null}]});
    fs::write(
        store.root.join("restore-journal.json"),
        serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    let _guard = store.lock().unwrap();
    assert_eq!(fs::read(store.root.join("cache.json")).unwrap(), original);
    assert!(!store.root.join("restore-journal.json").exists());
}
