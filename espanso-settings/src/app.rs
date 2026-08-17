use std::{
    cell::RefCell,
    fmt::Write as _,
    path::{Path, PathBuf},
    rc::Rc,
};

use anyhow::{Context, Result};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{
    scan_external_matches, AdoptPlan, AdoptService, BackupService, CategoryFilter,
    ConfigLocationStore, EspansoConfigValidator, ImportPlan, MatchRepository, MigrationService,
    ModelEffect, ModelMessage, ScaffoldService, SettingsLaunchOptions, SettingsModel, SettingsTab,
    UiMatch, UiMatchRepository,
};

pub fn run(options: SettingsLaunchOptions) -> Result<()> {
    let Some(_instance_guard) = crate::SettingsInstanceGuard::try_acquire(&options.paths.runtime)?
    else {
        crate::single_instance::notify_existing(&options.paths.runtime);
        return Ok(());
    };
    let window = crate::SettingsWindow::new()?;
    crate::single_instance::spawn_focus_listener(&options.paths.runtime, window.as_weak())?;

    // Both the repository and the active path are swapped in place when the
    // user switches directory, so every closure keeps working against the
    // directory that is current at click time rather than the one at startup.
    let repository = Rc::new(RefCell::new(UiMatchRepository::new(&options.paths.config)));
    let active_config = Rc::new(RefCell::new(options.paths.config.clone()));
    let model = Rc::new(RefCell::new(SettingsModel::default()));

    window.set_config_path(path_text(&options.paths.config));
    window.set_source_label(options.config_override_source.display_name().into());
    window.set_can_change_location(options.config_override_source.allows_persistent_change());
    reload_matches(&window, &model, &repository, &options.paths.config);

    bind_navigation(&window, Rc::clone(&model));
    bind_match_editor(&window, Rc::clone(&model), Rc::clone(&repository));
    bind_location(
        &window,
        options,
        Rc::clone(&model),
        Rc::clone(&repository),
        active_config,
    );

    window.run()?;
    Ok(())
}

/// Reloads both match lists from `config_root`.
///
/// Used for the initial load and after every directory switch so the two paths
/// cannot drift apart.
fn reload_matches(
    window: &crate::SettingsWindow,
    model: &Rc<RefCell<SettingsModel>>,
    repository: &Rc<RefCell<UiMatchRepository>>,
    config_root: &Path,
) {
    let (loaded, load_error) = match repository.borrow().load() {
        Ok(loaded) => (loaded, None),
        Err(error) => ((Vec::new(), Vec::new()), Some(error)),
    };
    let owned_path = repository.borrow().path().to_path_buf();
    {
        let mut model = model.borrow_mut();
        model.set_matches(loaded.0);
        model.set_categories(loaded.1);
        model.set_external(scan_external_matches(config_root, &owned_path));
    }
    if let Some(error) = load_error {
        window.set_error_message(format!("Cannot load matches; file unchanged: {error}").into());
    }
    refresh_category_models(window, &model.borrow());
    refresh_rows(window, &model.borrow());
}

fn bind_navigation(window: &crate::SettingsWindow, model: Rc<RefCell<SettingsModel>>) {
    let weak = window.as_weak();
    let switch_model = Rc::clone(&model);
    window.on_request_tab_switch(move |index| {
        let tab = if index == 0 {
            SettingsTab::Settings
        } else {
            SettingsTab::Configuration
        };
        let effect = switch_model
            .borrow_mut()
            .reduce(ModelMessage::SwitchTab(tab));
        if let Some(window) = weak.upgrade() {
            match effect {
                ModelEffect::ConfirmDiscard => window.set_confirm_discard_visible(true),
                ModelEffect::TabChanged(tab) => window.set_active_tab(tab_index(tab)),
                ModelEffect::None | ModelEffect::MatchesChanged => {}
            }
        }
    });

    let weak = window.as_weak();
    let confirm_model = Rc::clone(&model);
    window.on_confirm_discard(move || {
        let effect = confirm_model
            .borrow_mut()
            .reduce(ModelMessage::ConfirmDiscard);
        if let Some(window) = weak.upgrade() {
            window.set_confirm_discard_visible(false);
            if let ModelEffect::TabChanged(tab) = effect {
                window.set_active_tab(tab_index(tab));
            }
        }
    });

    let weak = window.as_weak();
    let cancel_model = Rc::clone(&model);
    window.on_cancel_discard(move || {
        cancel_model
            .borrow_mut()
            .reduce(ModelMessage::CancelDiscard);
        if let Some(window) = weak.upgrade() {
            window.set_confirm_discard_visible(false);
        }
    });

    let draft_model = Rc::clone(&model);
    window.on_draft_changed(move || {
        draft_model.borrow_mut().reduce(ModelMessage::DraftChanged);
    });
}

fn bind_match_editor(
    window: &crate::SettingsWindow,
    model: Rc<RefCell<SettingsModel>>,
    repository: Rc<RefCell<UiMatchRepository>>,
) {
    let weak = window.as_weak();
    let select_model = Rc::clone(&model);
    window.on_select_match(move |id| {
        let model = select_model.borrow();
        let selected = model
            .matches()
            .iter()
            .find(|item| item.id == id.as_str())
            .cloned();
        if let (Some(window), Some(selected)) = (weak.upgrade(), selected) {
            fill_editor(&window, &model, &selected);
            window.set_status_message("Match loaded; edit it and save".into());
        }
    });

    let weak = window.as_weak();
    window.on_new_match(move || {
        if let Some(window) = weak.upgrade() {
            clear_editor(&window);
            window.set_status_message("Creating a new match".into());
        }
    });

    let weak = window.as_weak();
    let save_model = Rc::clone(&model);
    let save_repository = Rc::clone(&repository);
    window.on_save_match(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut id = window.get_selected_id().to_string();
        if id.is_empty() {
            id = next_match_id();
        }
        let category_id =
            category_id_from_editor_index(&save_model.borrow(), window.get_editor_category_index());
        let candidate = UiMatch {
            id: id.clone(),
            trigger: window.get_trigger_text().to_string(),
            replace: window.get_replacement_text().to_string(),
            short_name: window.get_short_name_text().to_string(),
            description: window.get_description_text().to_string(),
            category_id,
        };
        let mut matches = save_model.borrow().matches().to_vec();
        if let Some(existing) = matches.iter_mut().find(|item| item.id == id) {
            *existing = candidate;
        } else {
            matches.push(candidate);
        }
        let categories = save_model.borrow().categories().to_vec();

        match save_repository.borrow().save(&matches, &categories) {
            Ok(()) => {
                save_model.borrow_mut().set_matches(matches);
                save_model.borrow_mut().reduce(ModelMessage::Saved);
                window.set_selected_id(id.into());
                window.set_error_message(SharedString::default());
                window.set_status_message("Match saved; Espanso will reload automatically".into());
                refresh_category_models(&window, &save_model.borrow());
                refresh_rows(&window, &save_model.borrow());
            }
            Err(error) => {
                window.set_error_message(error.to_string().into());
            }
        }
    });

    let weak = window.as_weak();
    let delete_model = Rc::clone(&model);
    window.on_delete_match(move |id| {
        let effect = delete_model
            .borrow_mut()
            .reduce(ModelMessage::Delete(id.to_string()));
        if let Some(window) = weak.upgrade() {
            if effect == ModelEffect::MatchesChanged {
                clear_editor(&window);
                window.set_undo_visible(true);
                window.set_status_message("Match removed; you can undo before saving".into());
                refresh_rows(&window, &delete_model.borrow());
            }
        }
    });

    let weak = window.as_weak();
    let undo_model = Rc::clone(&model);
    window.on_undo_delete(move || {
        let effect = undo_model.borrow_mut().reduce(ModelMessage::UndoDelete);
        if let Some(window) = weak.upgrade() {
            if effect == ModelEffect::MatchesChanged {
                window.set_undo_visible(false);
                window.set_status_message("Deletion undone".into());
                refresh_rows(&window, &undo_model.borrow());
            }
        }
    });

    let weak = window.as_weak();
    let commit_model = Rc::clone(&model);
    let commit_repository = Rc::clone(&repository);
    window.on_commit_deletion(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let matches = commit_model.borrow().matches().to_vec();
        let categories = commit_model.borrow().categories().to_vec();
        let outcome = commit_repository.borrow().save(&matches, &categories);
        match outcome {
            Ok(()) => {
                commit_model.borrow_mut().reduce(ModelMessage::Saved);
                window.set_undo_visible(false);
                window.set_error_message(SharedString::default());
                window.set_status_message("Deletion saved safely".into());
            }
            Err(error) => window.set_error_message(error.to_string().into()),
        }
    });

    let weak = window.as_weak();
    window.on_open_match_file(move |path| {
        if let Err(error) = open_path(Path::new(path.as_str())) {
            if let Some(window) = weak.upgrade() {
                window.set_error_message(error.to_string().into());
            }
        }
    });

    let weak = window.as_weak();
    let filter_model = Rc::clone(&model);
    window.on_apply_filter(move |filter| {
        filter_model
            .borrow_mut()
            .reduce(ModelMessage::FilterChanged(filter.to_string()));
        if let Some(window) = weak.upgrade() {
            refresh_rows(&window, &filter_model.borrow());
        }
    });

    let weak = window.as_weak();
    let category_filter_model = Rc::clone(&model);
    window.on_apply_category_filter(move |index| {
        let filter = category_filter_from_index(&category_filter_model.borrow(), index);
        category_filter_model
            .borrow_mut()
            .reduce(ModelMessage::CategoryFilterChanged(filter));
        if let Some(window) = weak.upgrade() {
            refresh_rows(&window, &category_filter_model.borrow());
        }
    });

    let weak = window.as_weak();
    let editor_category_model = Rc::clone(&model);
    window.on_editor_category_changed(move |index| {
        if let Some(window) = weak.upgrade() {
            let category_id = category_id_from_editor_index(&editor_category_model.borrow(), index);
            window.set_category_id(category_id.into());
            editor_category_model
                .borrow_mut()
                .reduce(ModelMessage::DraftChanged);
        }
    });

    let weak = window.as_weak();
    let add_category_model = Rc::clone(&model);
    let add_category_repository = Rc::clone(&repository);
    window.on_add_category(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let name = window.get_new_category_text().to_string();
        match add_category_model.borrow_mut().add_category(&name) {
            Ok(category) => {
                // Persist the category directory immediately so it survives even
                // if the user never saves the current draft match.
                let matches = add_category_model.borrow().matches().to_vec();
                let categories = add_category_model.borrow().categories().to_vec();
                if let Err(error) = add_category_repository.borrow().save(&matches, &categories) {
                    window.set_error_message(error.to_string().into());
                    return;
                }
                add_category_model.borrow_mut().reduce(ModelMessage::Saved);
                window.set_new_category_text(SharedString::default());
                window.set_category_id(category.id.clone().into());
                refresh_category_models(&window, &add_category_model.borrow());
                // Select the new category in the editor combo (index 0 is Uncategorized).
                let index = add_category_model
                    .borrow()
                    .categories()
                    .iter()
                    .position(|item| item.id == category.id)
                    .map_or(0, |position| (position + 1) as i32);
                window.set_editor_category_index(index);
                window.set_error_message(SharedString::default());
                window.set_status_message(format!("Category “{}” added", category.name).into());
                refresh_rows(&window, &add_category_model.borrow());
            }
            Err(error) => window.set_error_message(error.to_string().into()),
        }
    });
}

fn bind_location(
    window: &crate::SettingsWindow,
    options: SettingsLaunchOptions,
    model: Rc<RefCell<SettingsModel>>,
    repository: Rc<RefCell<UiMatchRepository>>,
    active_config: Rc<RefCell<PathBuf>>,
) {
    // Pending import lives across the preview → confirm click. Dropped on
    // cancel, successful restore, or a newer import plan.
    let pending_import: Rc<RefCell<Option<ImportPlan>>> = Rc::new(RefCell::new(None));

    // --- Open existing (adopt, zero writes). Also bound to the path "..." button. ---
    let weak = window.as_weak();
    let browse_config = Rc::clone(&active_config);
    let browse_store_path = options.location_store_path.clone();
    let browse_model = Rc::clone(&model);
    let browse_repository = Rc::clone(&repository);
    window.on_browse_config(move || {
        let current = browse_config.borrow().clone();
        let Some(selected) = rfd::FileDialog::new().set_directory(&current).pick_folder() else {
            return;
        };
        let Some(window) = weak.upgrade() else {
            return;
        };
        window.set_status_message("Checking the selected folder...".into());

        let result = AdoptService::plan(&current, &selected).and_then(|plan| {
            ConfigLocationStore::new(browse_store_path.clone()).save_atomic(&plan.destination)?;
            let message = adopted_message(&plan);
            Ok((plan.destination, message))
        });

        match result {
            Ok((destination, message)) => {
                apply_active_directory(
                    &window,
                    &browse_config,
                    &browse_repository,
                    &browse_model,
                    &destination,
                    &message,
                );
            }
            Err(error) => {
                window.set_status_message("Ready".into());
                window.set_error_message(
                    format!("Cannot open that directory: {error}; nothing was written.").into(),
                );
            }
        }
    });

    // --- Create new configuration in an empty folder. ---
    let weak = window.as_weak();
    let create_config = Rc::clone(&active_config);
    let create_store_path = options.location_store_path.clone();
    let create_model = Rc::clone(&model);
    let create_repository = Rc::clone(&repository);
    let templates = options.templates;
    window.on_create_config(move || {
        let current = create_config.borrow().clone();
        let Some(selected) = rfd::FileDialog::new().set_directory(&current).pick_folder() else {
            return;
        };
        let Some(window) = weak.upgrade() else {
            return;
        };
        window.set_status_message("Creating a default configuration...".into());

        let result = ScaffoldService::preflight(&current, &selected).and_then(|plan| {
            ScaffoldService::execute(&plan, &templates, &EspansoConfigValidator)?;
            ConfigLocationStore::new(create_store_path.clone()).save_atomic(&plan.destination)?;
            Ok(plan.destination)
        });

        match result {
            Ok(destination) => {
                apply_active_directory(
                    &window,
                    &create_config,
                    &create_repository,
                    &create_model,
                    &destination,
                    "Default configuration created; switched over. Restart Espanso.",
                );
            }
            Err(error) => {
                window.set_status_message("Ready".into());
                window.set_error_message(
                    format!("Cannot create configuration: {error}; nothing was written.").into(),
                );
            }
        }
    });

    // --- Export backup (read-only on the active config). ---
    let weak = window.as_weak();
    let export_config = Rc::clone(&active_config);
    window.on_export_backup(move || {
        let current = export_config.borrow().clone();
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Some(archive) = rfd::FileDialog::new()
            .set_file_name(BackupService::default_export_name())
            .add_filter("Espanso backup", &["zip"])
            .save_file()
        else {
            return;
        };
        window.set_status_message("Exporting backup...".into());
        match BackupService::export(&current, &archive) {
            Ok(summary) => {
                window.set_error_message(SharedString::default());
                window.set_status_message(
                    format!(
                        "Backup exported: {} file(s) ({} bytes) → {}.",
                        summary.file_count,
                        summary.byte_count,
                        summary.archive.display()
                    )
                    .into(),
                );
            }
            Err(error) => {
                window.set_status_message("Ready".into());
                window.set_error_message(
                    format!("Export failed: {error}; the configuration was not changed.").into(),
                );
            }
        }
    });

    // --- Import backup: pick archive + empty destination, then preview. ---
    let weak = window.as_weak();
    let import_pending = Rc::clone(&pending_import);
    window.on_import_backup(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Some(archive) = rfd::FileDialog::new()
            .add_filter("Espanso backup", &["zip"])
            .pick_file()
        else {
            return;
        };
        let Some(destination) = rfd::FileDialog::new()
            .set_title("Choose an empty folder to restore into")
            .pick_folder()
        else {
            return;
        };
        window.set_status_message("Checking the backup...".into());
        // Drop any previous staged import before planning a new one.
        if let Some(previous) = import_pending.borrow_mut().take() {
            BackupService::discard_import(&previous);
        }
        match BackupService::plan_import(&archive, &destination) {
            Ok(plan) => {
                let summary = format!(
                    "Ready to restore {} config file(s) and {} match(es) into {}. Nothing has been written yet.",
                    plan.adopt.config_count,
                    plan.adopt.match_count,
                    plan.destination.display()
                );
                window.set_import_summary(summary.into());
                window.set_import_ready(true);
                window.set_error_message(SharedString::default());
                window.set_status_message("Backup checked; confirm to restore.".into());
                *import_pending.borrow_mut() = Some(plan);
            }
            Err(error) => {
                window.set_import_summary(SharedString::default());
                window.set_import_ready(false);
                window.set_status_message("Ready".into());
                window.set_error_message(
                    format!("Cannot import that backup: {error}; nothing was written.").into(),
                );
            }
        }
    });

    let weak = window.as_weak();
    let confirm_pending = Rc::clone(&pending_import);
    let confirm_config = Rc::clone(&active_config);
    let confirm_model = Rc::clone(&model);
    let confirm_repository = Rc::clone(&repository);
    let confirm_store_path = options.location_store_path.clone();
    window.on_confirm_import(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Some(plan) = confirm_pending.borrow_mut().take() else {
            window.set_error_message("No backup is ready to restore.".into());
            return;
        };
        window.set_import_ready(false);
        window.set_status_message("Restoring backup...".into());
        let destination = plan.destination.clone();
        let result = BackupService::execute_import(&plan, &EspansoConfigValidator).and_then(|()| {
            ConfigLocationStore::new(confirm_store_path.clone()).save_atomic(&destination)?;
            Ok(plan.adopt.clone())
        });
        match result {
            Ok(adopt) => {
                window.set_import_summary(SharedString::default());
                let message = format!(
                    "Restored {} config file(s), {} match(es) to {}. Restart Espanso.",
                    adopt.config_count,
                    adopt.match_count,
                    destination.display()
                );
                apply_active_directory(
                    &window,
                    &confirm_config,
                    &confirm_repository,
                    &confirm_model,
                    &destination,
                    &message,
                );
            }
            Err(error) => {
                BackupService::discard_import(&plan);
                window.set_import_summary(SharedString::default());
                window.set_status_message("Ready".into());
                window.set_error_message(format!("Import failed: {error}").into());
            }
        }
    });

    let weak = window.as_weak();
    let cancel_pending = Rc::clone(&pending_import);
    window.on_cancel_import(move || {
        if let Some(plan) = cancel_pending.borrow_mut().take() {
            BackupService::discard_import(&plan);
        }
        if let Some(window) = weak.upgrade() {
            window.set_import_summary(SharedString::default());
            window.set_import_ready(false);
            window.set_status_message("Import cancelled.".into());
            window.set_error_message(SharedString::default());
        }
    });

    // --- Advanced: copy current tree to another empty folder. ---
    let weak = window.as_weak();
    let choose_config = Rc::clone(&active_config);
    window.on_choose_folder(move || {
        let current_config = choose_config.borrow().clone();
        let selected = rfd::FileDialog::new()
            .set_directory(&current_config)
            .pick_folder();
        if let (Some(window), Some(selected)) = (weak.upgrade(), selected) {
            match MigrationService::preflight(&current_config, &selected) {
                Ok(plan) => {
                    window.set_selected_path(path_text(&selected));
                    window.set_migration_summary(
                        format!(
                            "Will copy {} file(s) ({} bytes). The old directory will be kept.",
                            plan.file_count, plan.byte_count
                        )
                        .into(),
                    );
                    window.set_migration_ready(plan.conflicts.is_empty());
                    window.set_error_message(SharedString::default());
                }
                Err(error) => window.set_error_message(error.to_string().into()),
            }
        }
    });

    let weak = window.as_weak();
    let config_to_open = Rc::clone(&active_config);
    let migration_source = Rc::clone(&active_config);
    let store_path = options.location_store_path;
    window.on_migrate(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let destination = std::path::PathBuf::from(window.get_selected_path().to_string());
        window.set_migration_ready(false);
        window.set_status_message("Validating and copying the configuration...".into());
        // Read the source on the main thread: the worker below has to be Send,
        // so it cannot hold the shared active path itself.
        let source = migration_source.borrow().clone();
        let store_path = store_path.clone();
        let weak = window.as_weak();
        std::thread::spawn(move || {
            let result = MigrationService::preflight(&source, &destination).and_then(|plan| {
                MigrationService::execute(&plan, &EspansoConfigValidator)?;
                ConfigLocationStore::new(store_path).save_atomic(&destination)
            });
            let _ = weak.upgrade_in_event_loop(move |window| match result {
                Ok(()) => {
                    window.set_config_path(path_text(&destination));
                    window.set_status_message(
                        "Migration complete; old directory kept. Restart Espanso.".into(),
                    );
                    window.set_error_message(SharedString::default());
                }
                Err(error) => {
                    window.set_error_message(format!("Migration failed: {error}").into());
                }
            });
        });
    });

    let weak = window.as_weak();
    window.on_open_config(move || {
        let path = config_to_open.borrow().clone();
        if let Err(error) = open_path(&path) {
            if let Some(window) = weak.upgrade() {
                window.set_error_message(error.to_string().into());
            }
        }
    });
}

/// Switches the live Settings state over to `destination` after a successful
/// Open / Create / Import. Clears the advanced copy panel so it cannot target
/// a directory that is no longer empty.
fn apply_active_directory(
    window: &crate::SettingsWindow,
    active_config: &Rc<RefCell<PathBuf>>,
    repository: &Rc<RefCell<UiMatchRepository>>,
    model: &Rc<RefCell<SettingsModel>>,
    destination: &Path,
    message: &str,
) {
    window.set_config_path(path_text(destination));
    window.set_selected_path(SharedString::default());
    window.set_migration_summary(SharedString::default());
    window.set_migration_ready(false);
    window.set_import_summary(SharedString::default());
    window.set_import_ready(false);
    window.set_error_message(SharedString::default());
    window.set_status_message(message.into());

    *repository.borrow_mut() = UiMatchRepository::new(destination);
    *active_config.borrow_mut() = destination.to_path_buf();
    reload_matches(window, model, repository, destination);
}

fn adopted_message(plan: &AdoptPlan) -> String {
    let mut message = format!(
        "Imported the existing configuration: {} config file(s), {} match(es). Nothing in that folder was changed. Restart Espanso.",
        plan.config_count, plan.match_count
    );
    if !plan.warning_files.is_empty() {
        let _ = write!(
            message,
            " {} file(s) reported warnings; Espanso still loaded the configuration.",
            plan.warning_files.len()
        );
    }
    message
}

fn refresh_rows(window: &crate::SettingsWindow, model: &SettingsModel) {
    let mut rows = model
        .filtered_matches()
        .into_iter()
        .map(|item| {
            let title = if item.short_name.trim().is_empty() {
                item.trigger.clone()
            } else {
                item.short_name.clone()
            };
            let trigger_line = if item.short_name.trim().is_empty() {
                String::new()
            } else {
                item.trigger.clone()
            };
            crate::MatchRow {
                id: item.id.clone().into(),
                title: title.into(),
                trigger: trigger_line.into(),
                preview: item.replace.lines().next().unwrap_or_default().into(),
                category: model
                    .category_name(&item.category_id)
                    .unwrap_or_default()
                    .into(),
                source_path: SharedString::default(),
                source_label: SharedString::default(),
                note: SharedString::default(),
                editable: true,
            }
        })
        .collect::<Vec<_>>();
    // Editable entries first: those are the ones the user can act on here,
    // while the read-only ones are context.
    rows.extend(
        model
            .filtered_external()
            .into_iter()
            .map(|item| crate::MatchRow {
                id: SharedString::default(),
                title: item.trigger.clone().into(),
                trigger: SharedString::default(),
                preview: item.preview.clone().into(),
                category: SharedString::default(),
                source_path: path_text(&item.source),
                source_label: item.source_label.clone().into(),
                note: item.note.clone().into(),
                editable: false,
            }),
    );
    window.set_match_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
}

fn refresh_category_models(window: &crate::SettingsWindow, model: &SettingsModel) {
    let mut editor_names = vec![SharedString::from("Uncategorized")];
    editor_names.extend(
        model
            .categories()
            .iter()
            .map(|item| SharedString::from(item.name.as_str())),
    );
    window.set_category_names(ModelRc::from(Rc::new(VecModel::from(editor_names))));

    let mut filter_names = vec![
        SharedString::from("All categories"),
        SharedString::from("Uncategorized"),
    ];
    filter_names.extend(
        model
            .categories()
            .iter()
            .map(|item| SharedString::from(item.name.as_str())),
    );
    window.set_category_filter_names(ModelRc::from(Rc::new(VecModel::from(filter_names))));

    // Keep the filter combo index aligned with the current model filter.
    let filter_index = match model.category_filter() {
        CategoryFilter::All => 0,
        CategoryFilter::Uncategorized => 1,
        CategoryFilter::Id(id) => model
            .categories()
            .iter()
            .position(|item| item.id == *id)
            .map_or(0, |position| (position + 2) as i32),
    };
    window.set_category_filter_index(filter_index);
}

fn fill_editor(window: &crate::SettingsWindow, model: &SettingsModel, selected: &UiMatch) {
    window.set_selected_id(selected.id.clone().into());
    window.set_short_name_text(selected.short_name.clone().into());
    window.set_trigger_text(selected.trigger.clone().into());
    window.set_replacement_text(selected.replace.clone().into());
    window.set_description_text(selected.description.clone().into());
    window.set_category_id(selected.category_id.clone().into());
    window.set_editor_category_index(editor_index_for_category(model, &selected.category_id));
}

fn clear_editor(window: &crate::SettingsWindow) {
    window.set_selected_id(SharedString::default());
    window.set_short_name_text(SharedString::default());
    window.set_trigger_text(SharedString::default());
    window.set_replacement_text(SharedString::default());
    window.set_description_text(SharedString::default());
    window.set_category_id(SharedString::default());
    window.set_editor_category_index(0);
}

fn category_id_from_editor_index(model: &SettingsModel, index: i32) -> String {
    if index <= 0 {
        return String::new();
    }
    model
        .categories()
        .get((index as usize).saturating_sub(1))
        .map(|item| item.id.clone())
        .unwrap_or_default()
}

fn editor_index_for_category(model: &SettingsModel, category_id: &str) -> i32 {
    let id = category_id.trim();
    if id.is_empty() {
        return 0;
    }
    model
        .categories()
        .iter()
        .position(|item| item.id == id)
        .map_or(0, |position| (position + 1) as i32)
}

fn category_filter_from_index(model: &SettingsModel, index: i32) -> CategoryFilter {
    match index {
        i if i <= 0 => CategoryFilter::All,
        1 => CategoryFilter::Uncategorized,
        i => model
            .categories()
            .get((i as usize).saturating_sub(2))
            .map(|item| CategoryFilter::Id(item.id.clone()))
            .unwrap_or(CategoryFilter::All),
    }
}

fn path_text(path: &Path) -> SharedString {
    path.to_string_lossy().to_string().into()
}

const fn tab_index(tab: SettingsTab) -> i32 {
    match tab {
        SettingsTab::Settings => 0,
        SettingsTab::Configuration => 1,
    }
}

fn next_match_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("ui-{value}")
}

fn open_path(path: &Path) -> Result<()> {
    opener::open(path).with_context(|| format!("unable to open {}", path.display()))
}
