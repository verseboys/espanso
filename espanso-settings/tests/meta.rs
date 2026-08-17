use espanso_settings::{
    create_category, document_from_matches, Category, MatchMeta, UiMatch, UiMetaDocument,
    UiMetaStore,
};
use tempdir::TempDir;

#[test]
fn meta_store_round_trip() {
    let root = TempDir::new("settings-meta-store").unwrap();
    std::fs::create_dir_all(root.path().join("match")).unwrap();
    let store = UiMetaStore::new(root.path());
    let document = UiMetaDocument {
        version: 1,
        categories: vec![Category {
            id: "cat-1".to_string(),
            name: "个人".to_string(),
            order: 0,
        }],
        matches: [(
            "ui-1".to_string(),
            MatchMeta {
                description: "note".to_string(),
                category_id: "cat-1".to_string(),
            },
        )]
        .into_iter()
        .collect(),
    };
    store.save(&document).unwrap();
    assert_eq!(store.load().unwrap(), document);
}

#[test]
fn create_category_normalizes_and_rejects_blank() {
    assert!(create_category(&[], "  ").is_err());
    let category = create_category(&[], "  邮件  ").unwrap();
    assert_eq!(category.name, "邮件");
    assert!(create_category(std::slice::from_ref(&category), "邮件").is_err());
}

#[test]
fn document_from_matches_drops_empty_meta() {
    let categories = vec![Category {
        id: "cat-1".to_string(),
        name: "A".to_string(),
        order: 0,
    }];
    let matches = vec![
        UiMatch {
            id: "with".to_string(),
            trigger: ":a".to_string(),
            replace: "a".to_string(),
            short_name: String::new(),
            description: "d".to_string(),
            category_id: "cat-1".to_string(),
        },
        UiMatch {
            id: "empty".to_string(),
            trigger: ":b".to_string(),
            replace: "b".to_string(),
            short_name: String::new(),
            description: String::new(),
            category_id: String::new(),
        },
    ];
    let document = document_from_matches(&categories, &matches);
    assert!(document.matches.contains_key("with"));
    assert!(!document.matches.contains_key("empty"));
}
