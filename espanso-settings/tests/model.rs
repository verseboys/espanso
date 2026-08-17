use espanso_settings::{
    Category, CategoryFilter, ModelEffect, ModelMessage, SettingsModel, SettingsTab, UiMatch,
};

#[test]
fn dirty_tab_switch_requires_confirmation() {
    let mut model = SettingsModel::default();
    assert_eq!(model.reduce(ModelMessage::DraftChanged), ModelEffect::None);
    assert!(model.is_dirty());

    assert_eq!(
        model.reduce(ModelMessage::SwitchTab(SettingsTab::Configuration)),
        ModelEffect::ConfirmDiscard
    );
    assert_eq!(model.active_tab(), SettingsTab::Settings);

    assert_eq!(
        model.reduce(ModelMessage::ConfirmDiscard),
        ModelEffect::TabChanged(SettingsTab::Configuration)
    );
    assert_eq!(model.active_tab(), SettingsTab::Configuration);
    assert!(!model.is_dirty());
}

#[test]
fn filter_is_case_insensitive_and_checks_trigger_replacement_and_meta() {
    let mut model = SettingsModel::default();
    model.set_matches(vec![
        UiMatch {
            id: "one".to_string(),
            trigger: ":Hello".to_string(),
            replace: "世界".to_string(),
            short_name: "Greeting".to_string(),
            description: "morning note".to_string(),
            category_id: String::new(),
        },
        ui_match("two", ":bye", "Good Night"),
    ]);

    model.reduce(ModelMessage::FilterChanged("hello".to_string()));
    assert_eq!(model.filtered_matches()[0].id, "one");

    model.reduce(ModelMessage::FilterChanged("night".to_string()));
    assert_eq!(model.filtered_matches()[0].id, "two");

    model.reduce(ModelMessage::FilterChanged("greeting".to_string()));
    assert_eq!(model.filtered_matches()[0].id, "one");

    model.reduce(ModelMessage::FilterChanged("morning".to_string()));
    assert_eq!(model.filtered_matches()[0].id, "one");
}

#[test]
fn category_filter_hides_non_matching_owned_matches() {
    let mut model = SettingsModel::default();
    model.set_categories(vec![Category {
        id: "cat-a".to_string(),
        name: "A".to_string(),
        order: 0,
    }]);
    model.set_matches(vec![
        UiMatch {
            id: "one".to_string(),
            trigger: ":one".to_string(),
            replace: "1".to_string(),
            short_name: String::new(),
            description: String::new(),
            category_id: "cat-a".to_string(),
        },
        ui_match("two", ":two", "2"),
    ]);

    model.reduce(ModelMessage::CategoryFilterChanged(
        CategoryFilter::Uncategorized,
    ));
    let filtered = model.filtered_matches();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "two");

    model.reduce(ModelMessage::CategoryFilterChanged(CategoryFilter::Id(
        "cat-a".to_string(),
    )));
    let filtered = model.filtered_matches();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "one");
}

#[test]
fn add_category_rejects_duplicates_and_marks_dirty() {
    let mut model = SettingsModel::default();
    let created = model.add_category("工作").unwrap();
    assert_eq!(created.name, "工作");
    assert!(model.is_dirty());
    assert!(model.add_category("工作").is_err());
    assert!(model.add_category("  ").is_err());
}

#[test]
fn deleted_match_can_be_undone_before_save() {
    let mut model = SettingsModel::default();
    model.set_matches(vec![ui_match("one", ":one", "first")]);

    assert_eq!(
        model.reduce(ModelMessage::Delete("one".to_string())),
        ModelEffect::MatchesChanged
    );
    assert!(model.matches().is_empty());
    assert!(model.is_dirty());

    assert_eq!(
        model.reduce(ModelMessage::UndoDelete),
        ModelEffect::MatchesChanged
    );
    assert_eq!(model.matches()[0].id, "one");
}

#[test]
fn discarding_dirty_navigation_restores_pending_deletion() {
    let mut model = SettingsModel::default();
    model.set_matches(vec![ui_match("one", ":one", "first")]);
    model.reduce(ModelMessage::Delete("one".to_string()));
    model.reduce(ModelMessage::SwitchTab(SettingsTab::Configuration));

    assert_eq!(
        model.reduce(ModelMessage::ConfirmDiscard),
        ModelEffect::TabChanged(SettingsTab::Configuration)
    );
    assert_eq!(model.matches()[0].id, "one");
    assert!(!model.is_dirty());
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
