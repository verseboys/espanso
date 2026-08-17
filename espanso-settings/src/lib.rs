mod adopt;
mod backup;
mod inventory;
mod location;
mod matches;
mod meta;
mod migration;
mod model;
mod scaffold;
mod single_instance;

#[cfg(feature = "ui")]
mod app;

#[cfg(feature = "ui")]
slint::include_modules!();

pub use adopt::{AdoptPlan, AdoptService};
pub use backup::{
    BackupManifest, BackupService, ExportSummary, ImportPlan, BACKUP_FORMAT, BACKUP_FORMAT_VERSION,
    BACKUP_MANIFEST_NAME,
};
pub use inventory::{scan_external_matches, ExternalMatch};
pub use location::{
    resolve_config_location, ConfigLocationStore, ConfigPathSource, ResolvedConfigLocation,
};
pub use matches::{MatchRepository, UiMatch, UiMatchRepository};
pub use meta::{
    create_category, document_from_matches, next_category_id, Category, MatchMeta, UiMetaDocument,
    UiMetaStore,
};
pub use migration::{ConfigValidator, EspansoConfigValidator, MigrationPlan, MigrationService};
pub use model::{CategoryFilter, ModelEffect, ModelMessage, SettingsModel, SettingsTab};
pub use scaffold::{ConfigTemplates, ScaffoldPlan, ScaffoldService};
pub use single_instance::SettingsInstanceGuard;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPaths {
    pub config: PathBuf,
    pub packages: PathBuf,
    pub runtime: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLaunchOptions {
    pub paths: SettingsPaths,
    pub location_store_path: PathBuf,
    pub config_override_source: ConfigPathSource,
    pub templates: ConfigTemplates,
}

#[cfg(feature = "ui")]
pub fn run(options: SettingsLaunchOptions) -> anyhow::Result<()> {
    app::run(options)
}

#[cfg(not(feature = "ui"))]
pub fn run(_options: SettingsLaunchOptions) -> anyhow::Result<()> {
    anyhow::bail!("espanso-settings was built without the ui feature")
}
