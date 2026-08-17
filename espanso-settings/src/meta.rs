use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};

/// One user-defined category stored in `match/ui-meta.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub order: u32,
}

/// Per-match UI metadata that does not belong in the espanso match engine schema.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MatchMeta {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiMetaDocument {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub matches: BTreeMap<String, MatchMeta>,
}

fn default_version() -> u32 {
    1
}

impl Default for UiMetaDocument {
    fn default() -> Self {
        Self {
            version: 1,
            categories: Vec::new(),
            matches: BTreeMap::new(),
        }
    }
}

/// Loads and saves Settings UI metadata beside the managed match file.
#[derive(Debug, Clone)]
pub struct UiMetaStore {
    path: PathBuf,
}

impl UiMetaStore {
    pub fn new(config_root: &Path) -> Self {
        Self {
            path: config_root.join("match").join("ui-meta.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<UiMetaDocument> {
        if !self.path.exists() {
            return Ok(UiMetaDocument::default());
        }
        let content =
            fs::read_to_string(&self.path).with_context(|| "unable to read UI match metadata")?;
        let document: UiMetaDocument =
            serde_json::from_str(&content).with_context(|| "unable to parse UI match metadata")?;
        Ok(document)
    }

    pub fn save(&self, document: &UiMetaDocument) -> Result<()> {
        validate_document(document)?;
        let parent = self
            .path
            .parent()
            .context("UI match metadata file has no parent")?;
        fs::create_dir_all(parent)?;
        let payload = serde_json::to_vec_pretty(document)
            .with_context(|| "unable to serialize UI match metadata")?;
        AtomicFile::new(&self.path, AllowOverwrite)
            .write(|file| {
                file.write_all(&payload)?;
                file.write_all(b"\n")?;
                file.sync_all()
            })
            .with_context(|| "unable to save UI match metadata atomically")?;
        Ok(())
    }
}

/// Builds a metadata document from the live match list and category directory.
pub fn document_from_matches(
    categories: &[Category],
    matches: &[crate::UiMatch],
) -> UiMetaDocument {
    let mut match_meta = BTreeMap::new();
    for item in matches {
        let description = item.description.trim();
        let category_id = item.category_id.trim();
        if description.is_empty() && category_id.is_empty() {
            continue;
        }
        match_meta.insert(
            item.id.clone(),
            MatchMeta {
                description: description.to_string(),
                category_id: category_id.to_string(),
            },
        );
    }
    UiMetaDocument {
        version: 1,
        categories: categories.to_vec(),
        matches: match_meta,
    }
}

pub fn next_category_id() -> String {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("cat-{value}")
}

/// Creates a category after validating the display name is unique (case-insensitive).
pub fn create_category(categories: &[Category], name: &str) -> Result<Category> {
    let name = name.trim();
    if name.is_empty() {
        bail!("category name must not be blank");
    }
    if name.contains(['\n', '\r', '\0']) {
        bail!("category name must be a single line without NUL characters");
    }
    let lowered = name.to_lowercase();
    if categories
        .iter()
        .any(|item| item.name.to_lowercase() == lowered)
    {
        bail!("a category named {name:?} already exists");
    }
    let order = categories
        .iter()
        .map(|item| item.order)
        .max()
        .map_or(0, |value| value.saturating_add(1));
    Ok(Category {
        id: next_category_id(),
        name: name.to_string(),
        order,
    })
}

fn validate_document(document: &UiMetaDocument) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    for category in &document.categories {
        let name = category.name.trim();
        if name.is_empty() {
            bail!("category name must not be blank");
        }
        if category.id.trim().is_empty() {
            bail!("category id must not be blank");
        }
        if !ids.insert(category.id.clone()) {
            bail!("duplicate category id: {}", category.id);
        }
        if !names.insert(name.to_lowercase()) {
            bail!("duplicate category name: {name}");
        }
    }
    let known: std::collections::HashSet<_> = document
        .categories
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    for (match_id, meta) in &document.matches {
        if match_id.trim().is_empty() {
            bail!("match metadata key must not be blank");
        }
        let category_id = meta.category_id.trim();
        if !category_id.is_empty() && !known.contains(category_id) {
            // Orphan category references are cleared by the repository before save;
            // reject if they still slip through so the file stays consistent.
            bail!("match {match_id} references unknown category {category_id}");
        }
    }
    Ok(())
}
