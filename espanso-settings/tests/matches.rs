use std::fs;

use espanso_settings::{Category, MatchRepository, UiMatch, UiMatchRepository};
use tempdir::TempDir;

#[test]
fn round_trip_preserves_multiline_unicode_and_long_text() {
    let root = TempDir::new("settings-matches-roundtrip").unwrap();
    create_config(root.path());
    let repository = UiMatchRepository::new(root.path());
    let long = format!("第一行 🦀\n{}\n最后一行", "x".repeat(100_000));
    let matches = vec![UiMatch {
        id: "hello".to_string(),
        trigger: ":你好".to_string(),
        replace: long.clone(),
        short_name: "问候".to_string(),
        description: "多行说明".to_string(),
        category_id: String::new(),
    }];

    repository.save(&matches, &[]).unwrap();
    let (loaded, categories) = repository.load().unwrap();

    assert_eq!(loaded, matches);
    assert!(categories.is_empty());
    assert_eq!(loaded[0].replace, long);
    assert!(repository.path().ends_with("match/ui.yml"));
}

#[test]
fn short_name_description_and_category_round_trip_via_meta_json() {
    let root = TempDir::new("settings-matches-meta").unwrap();
    create_config(root.path());
    let repository = UiMatchRepository::new(root.path());
    let categories = vec![Category {
        id: "cat-work".to_string(),
        name: "工作".to_string(),
        order: 0,
    }];
    let matches = vec![UiMatch {
        id: "ui-1".to_string(),
        trigger: ":sig".to_string(),
        replace: "Best regards".to_string(),
        short_name: "工单签名".to_string(),
        description: "客服落款".to_string(),
        category_id: "cat-work".to_string(),
    }];

    repository.save(&matches, &categories).unwrap();
    assert!(repository.meta_path().exists());

    let (loaded, loaded_categories) = repository.load().unwrap();
    assert_eq!(loaded, matches);
    assert_eq!(loaded_categories, categories);

    let yml = fs::read_to_string(repository.path()).unwrap();
    assert!(
        yml.contains("ui_id: ui-1") || yml.contains("ui_id: \"ui-1\"") || yml.contains("ui_id:")
    );
    assert!(yml.contains("工单签名"));
    let meta = fs::read_to_string(repository.meta_path()).unwrap();
    assert!(meta.contains("cat-work"));
    assert!(meta.contains("客服落款"));
}

#[test]
fn legacy_label_as_id_loads_without_short_name() {
    let root = TempDir::new("settings-matches-legacy").unwrap();
    create_config(root.path());
    fs::write(
        root.path().join("match/ui.yml"),
        "matches:\n  - label: ui-legacy\n    trigger: ':old'\n    replace: classic\n",
    )
    .unwrap();
    let repository = UiMatchRepository::new(root.path());
    let (loaded, _) = repository.load().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "ui-legacy");
    assert!(loaded[0].short_name.is_empty());
    assert_eq!(loaded[0].trigger, ":old");
}

#[test]
fn invalid_matches_never_replace_the_last_valid_file() {
    let root = TempDir::new("settings-matches-validation").unwrap();
    create_config(root.path());
    let repository = UiMatchRepository::new(root.path());
    let valid = vec![ui_match("valid", ":ok", "works")];
    repository.save(&valid, &[]).unwrap();
    let before = fs::read(repository.path()).unwrap();

    let duplicate = vec![
        ui_match("one", ":same", "one"),
        ui_match("two", ":same", "two"),
    ];
    assert!(repository.save(&duplicate, &[]).is_err());
    assert_eq!(fs::read(repository.path()).unwrap(), before);

    let newline_trigger = vec![UiMatch {
        id: "bad".to_string(),
        trigger: ":bad\ntrigger".to_string(),
        replace: "bad".to_string(),
        short_name: String::new(),
        description: String::new(),
        category_id: String::new(),
    }];
    assert!(repository.save(&newline_trigger, &[]).is_err());
    assert_eq!(fs::read(repository.path()).unwrap(), before);
}

#[test]
fn invalid_external_yaml_blocks_save_and_preserves_managed_file() {
    let root = TempDir::new("settings-matches-external-invalid").unwrap();
    create_config(root.path());
    let repository = UiMatchRepository::new(root.path());
    let original = vec![ui_match("original", ":original", "safe")];
    repository.save(&original, &[]).unwrap();
    let before = fs::read(repository.path()).unwrap();
    fs::write(root.path().join("match/broken.yml"), "matches: [\n").unwrap();

    let changed = vec![ui_match("changed", ":changed", "must not publish")];
    assert!(repository.save(&changed, &[]).is_err());
    assert_eq!(fs::read(repository.path()).unwrap(), before);
}

#[test]
fn candidate_is_validated_with_the_complete_configuration_before_publish() {
    let root = TempDir::new("settings-matches-candidate-validation").unwrap();
    create_config(root.path());
    fs::write(
        root.path().join("match/external.yml"),
        "matches:\n  - trigger: ':external'\n    replace: external\n",
    )
    .unwrap();
    let repository = UiMatchRepository::new(root.path());
    let original = vec![ui_match("original", ":original", "safe")];
    repository.save(&original, &[]).unwrap();
    let before = fs::read(repository.path()).unwrap();

    let conflicting = vec![ui_match("conflicting", ":external", "must not publish")];
    assert!(repository.save(&conflicting, &[]).is_err());
    assert_eq!(fs::read(repository.path()).unwrap(), before);
}

fn ui_match(id: &str, trigger: &str, replace: &str) -> UiMatch {
    UiMatch {
        id: id.to_string(),
        trigger: trigger.to_string(),
        replace: replace.to_string(),
        short_name: String::new(),
        description: String::new(),
        category_id: String::new(),
    }
}

fn create_config(root: &std::path::Path) {
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("match")).unwrap();
    fs::write(root.join("config/default.yml"), "matches: []\n").unwrap();
}
