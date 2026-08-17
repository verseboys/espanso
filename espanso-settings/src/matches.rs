use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::{
    document_from_matches, migration::copy_tree, Category, ConfigValidator, EspansoConfigValidator,
    MatchMeta, UiMetaStore,
};

const MANAGED_HEADER: &str =
    "# Managed by Espanso Settings. Advanced rules may be edited in other files.\n";
const MAX_REPLACEMENT_CHARS: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiMatch {
    pub id: String,
    pub trigger: String,
    pub replace: String,
    /// Human-readable short name; stored as espanso `label` when non-empty.
    pub short_name: String,
    /// Free-form note; stored in `match/ui-meta.json`.
    pub description: String,
    /// Category id reference; empty means uncategorized.
    pub category_id: String,
}

pub trait MatchRepository {
    fn load(&self) -> Result<(Vec<UiMatch>, Vec<Category>)>;
    fn save(&self, matches: &[UiMatch], categories: &[Category]) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct UiMatchRepository {
    config_root: PathBuf,
    path: PathBuf,
    meta: UiMetaStore,
}

#[derive(Debug, Serialize, Deserialize)]
struct MatchDocument {
    #[serde(default)]
    matches: Vec<StoredMatch>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredMatch {
    /// Stable Settings identity. Unknown to the espanso engine (ignored).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui_id: Option<String>,
    /// Espanso search-bar label; used as the UI short name when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    trigger: String,
    replace: String,
}

#[derive(Debug, Default, Deserialize)]
struct TriggerScanDocument {
    #[serde(default)]
    matches: Vec<TriggerScanMatch>,
}

#[derive(Debug, Default, Deserialize)]
struct TriggerScanMatch {
    trigger: Option<String>,
    #[serde(default)]
    triggers: Vec<String>,
}

impl UiMatchRepository {
    pub fn new(config_root: &Path) -> Self {
        Self {
            config_root: config_root.to_path_buf(),
            path: config_root.join("match").join("ui.yml"),
            meta: UiMetaStore::new(config_root),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn meta_path(&self) -> &Path {
        self.meta.path()
    }
}

impl MatchRepository for UiMatchRepository {
    fn load(&self) -> Result<(Vec<UiMatch>, Vec<Category>)> {
        let meta = self.meta.load()?;
        let known_categories: HashSet<String> =
            meta.categories.iter().map(|item| item.id.clone()).collect();

        if !self.path.exists() {
            return Ok((Vec::new(), meta.categories));
        }
        let content =
            fs::read_to_string(&self.path).with_context(|| "unable to read UI-managed matches")?;
        let document: MatchDocument = serde_norway::from_str(&content)
            .with_context(|| "unable to parse UI-managed matches")?;
        let matches = document
            .matches
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let (id, short_name) = resolve_identity(index, item.ui_id, item.label);
                let entry = meta.matches.get(&id).cloned().unwrap_or_default();
                let category_id = if !entry.category_id.is_empty()
                    && known_categories.contains(&entry.category_id)
                {
                    entry.category_id
                } else {
                    String::new()
                };
                UiMatch {
                    id,
                    trigger: item.trigger,
                    replace: item.replace,
                    short_name,
                    description: entry.description,
                    category_id,
                }
            })
            .collect::<Vec<_>>();
        validate_matches(&matches)?;
        Ok((matches, meta.categories))
    }

    fn save(&self, matches: &[UiMatch], categories: &[Category]) -> Result<()> {
        validate_matches(matches)?;
        self.validate_external_trigger_conflicts(matches)?;
        if self.config_root.exists() {
            EspansoConfigValidator
                .validate(&self.config_root)
                .with_context(|| "existing Espanso configuration is invalid")?;
        }

        let document = MatchDocument {
            matches: matches
                .iter()
                .map(|item| StoredMatch {
                    ui_id: Some(item.id.clone()),
                    label: non_empty(item.short_name.trim()),
                    trigger: item.trigger.clone(),
                    replace: item.replace.clone(),
                })
                .collect(),
        };
        let mut payload = MANAGED_HEADER.as_bytes().to_vec();
        payload.extend(serde_norway::to_string(&document)?.as_bytes());

        self.validate_candidate(&payload)?;

        let parent = self
            .path
            .parent()
            .context("managed match file has no parent")?;
        fs::create_dir_all(parent)?;
        if self.path.exists() {
            fs::copy(&self.path, self.path.with_extension("yml.backup"))
                .with_context(|| "unable to back up the previous managed match file")?;
        }
        AtomicFile::new(&self.path, AllowOverwrite)
            .write(|file| {
                file.write_all(&payload)?;
                file.sync_all()
            })
            .with_context(|| "unable to save UI-managed matches atomically")?;

        // Meta is non-engine-critical: write after yml so a failed meta write can be
        // retried without re-validating the match set. Clear orphan category refs first.
        let mut meta_doc = document_from_matches(categories, matches);
        let known: HashSet<_> = meta_doc
            .categories
            .iter()
            .map(|item| item.id.clone())
            .collect();
        for entry in meta_doc.matches.values_mut() {
            if !entry.category_id.is_empty() && !known.contains(&entry.category_id) {
                entry.category_id.clear();
            }
        }
        // Drop empty match meta entries after cleanup.
        meta_doc.matches.retain(|_, value| {
            !value.description.trim().is_empty() || !value.category_id.trim().is_empty()
        });
        self.meta
            .save(&meta_doc)
            .with_context(|| "matches were saved but UI metadata could not be written")?;
        Ok(())
    }
}

impl UiMatchRepository {
    fn validate_external_trigger_conflicts(&self, matches: &[UiMatch]) -> Result<()> {
        let candidate_triggers = matches
            .iter()
            .map(|item| item.trigger.trim())
            .collect::<HashSet<_>>();
        let match_root = self.config_root.join("match");
        if !match_root.exists() {
            return Ok(());
        }

        for entry in WalkDir::new(match_root).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() || entry.path() == self.path {
                continue;
            }
            let extension = entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if !extension.eq_ignore_ascii_case("yml") && !extension.eq_ignore_ascii_case("yaml") {
                continue;
            }

            let content = fs::read_to_string(entry.path())?;
            let document: TriggerScanDocument = serde_norway::from_str(&content)
                .with_context(|| format!("unable to parse {}", entry.path().display()))?;
            for item in document.matches {
                let external_triggers = item.trigger.into_iter().chain(item.triggers);
                for trigger in external_triggers {
                    if candidate_triggers.contains(trigger.trim()) {
                        bail!(
                            "trigger {trigger:?} already exists in {}",
                            entry.path().display()
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_candidate(&self, payload: &[u8]) -> Result<()> {
        let parent = self
            .config_root
            .parent()
            .context("configuration root has no parent")?;
        let root_name = self
            .config_root
            .file_name()
            .context("configuration root has no directory name")?
            .to_string_lossy();
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let staging = parent.join(format!(
            ".{root_name}.espanso-settings-validation-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&staging)
            .with_context(|| "unable to create candidate validation directory")?;

        let validation = (|| -> Result<()> {
            copy_tree(&self.config_root, &staging)?;
            let candidate_path = staging.join("match").join("ui.yml");
            if let Some(parent) = candidate_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut candidate = fs::File::create(&candidate_path)?;
            candidate.write_all(payload)?;
            candidate.sync_all()?;
            EspansoConfigValidator
                .validate(&staging)
                .with_context(|| "candidate matches conflict with the complete configuration")
        })();
        let cleanup = fs::remove_dir_all(&staging)
            .with_context(|| "unable to remove candidate validation directory");

        validation?;
        cleanup?;
        Ok(())
    }
}

fn resolve_identity(
    index: usize,
    ui_id: Option<String>,
    label: Option<String>,
) -> (String, String) {
    if let Some(id) = ui_id
        .map(|value| value.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        let short_name = label
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_default();
        return (id, short_name);
    }
    match label
        .map(|value| value.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(label) if looks_like_internal_id(&label) => (label, String::new()),
        Some(label) => (format!("ui-match-{index}"), label),
        None => (format!("ui-match-{index}"), String::new()),
    }
}

fn looks_like_internal_id(value: &str) -> bool {
    value.starts_with("ui-") || value.starts_with("ui-match-")
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn validate_matches(matches: &[UiMatch]) -> Result<()> {
    let mut triggers = HashSet::new();
    for item in matches {
        let normalized = item.trigger.trim();
        if normalized.is_empty() {
            bail!("trigger must not be blank");
        }
        if item.trigger.contains(['\n', '\r', '\0']) {
            bail!("trigger must be a single line without NUL characters");
        }
        if !triggers.insert(normalized.to_string()) {
            bail!("duplicate trigger: {normalized}");
        }
        if item.replace.chars().count() > MAX_REPLACEMENT_CHARS {
            bail!("replacement exceeds the supported character limit");
        }
        if item.short_name.contains(['\n', '\r', '\0']) {
            bail!("short name must be a single line without NUL characters");
        }
        if item.description.contains('\0') {
            bail!("description must not contain NUL characters");
        }
    }
    Ok(())
}

// Re-export for callers that only need the empty meta shape in tests.
#[allow(dead_code)]
pub(crate) type LoadedMatchMeta = MatchMeta;
