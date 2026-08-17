use anyhow::Result;

use crate::{create_category, Category, ExternalMatch, UiMatch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Settings,
    Configuration,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CategoryFilter {
    #[default]
    All,
    Uncategorized,
    Id(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelMessage {
    SwitchTab(SettingsTab),
    ConfirmDiscard,
    CancelDiscard,
    DraftChanged,
    Saved,
    FilterChanged(String),
    CategoryFilterChanged(CategoryFilter),
    Delete(String),
    UndoDelete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEffect {
    None,
    ConfirmDiscard,
    TabChanged(SettingsTab),
    MatchesChanged,
}

#[derive(Debug, Default)]
pub struct SettingsModel {
    active_tab: SettingsTab,
    pending_tab: Option<SettingsTab>,
    dirty: bool,
    filter: String,
    category_filter: CategoryFilter,
    matches: Vec<UiMatch>,
    categories: Vec<Category>,
    /// Matches from files Settings does not own. Kept in a separate list rather
    /// than mixed into `matches` so that no save path can reach them: saving
    /// only ever walks `matches`, so copying a foreign file into `ui.yml` is not
    /// something the code can do by accident.
    external: Vec<ExternalMatch>,
    deleted: Option<(usize, UiMatch)>,
}

impl SettingsModel {
    pub const fn active_tab(&self) -> SettingsTab {
        self.active_tab
    }

    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn matches(&self) -> &[UiMatch] {
        &self.matches
    }

    pub fn set_matches(&mut self, matches: Vec<UiMatch>) {
        self.matches = matches;
        self.deleted = None;
        self.dirty = false;
    }

    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    pub fn set_categories(&mut self, mut categories: Vec<Category>) {
        categories.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        self.categories = categories;
    }

    pub fn category_filter(&self) -> &CategoryFilter {
        &self.category_filter
    }

    pub fn add_category(&mut self, name: &str) -> Result<Category> {
        let category = create_category(&self.categories, name)?;
        self.categories.push(category.clone());
        self.categories.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        self.dirty = true;
        Ok(category)
    }

    pub fn external(&self) -> &[ExternalMatch] {
        &self.external
    }

    pub fn set_external(&mut self, external: Vec<ExternalMatch>) {
        self.external = external;
    }

    pub fn filtered_matches(&self) -> Vec<&UiMatch> {
        let filter = self.filter.to_lowercase();
        self.matches
            .iter()
            .filter(|item| self.matches_category_filter(item))
            .filter(|item| {
                filter.is_empty()
                    || item.trigger.to_lowercase().contains(&filter)
                    || item.replace.to_lowercase().contains(&filter)
                    || item.short_name.to_lowercase().contains(&filter)
                    || item.description.to_lowercase().contains(&filter)
            })
            .collect()
    }

    pub fn filtered_external(&self) -> Vec<&ExternalMatch> {
        // Category filter only applies to Settings-owned matches.
        if !matches!(self.category_filter, CategoryFilter::All) {
            return Vec::new();
        }
        let filter = self.filter.to_lowercase();
        self.external
            .iter()
            .filter(|item| {
                filter.is_empty()
                    || item.trigger.to_lowercase().contains(&filter)
                    || item.preview.to_lowercase().contains(&filter)
                    || item.source_label.to_lowercase().contains(&filter)
            })
            .collect()
    }

    fn matches_category_filter(&self, item: &UiMatch) -> bool {
        match &self.category_filter {
            CategoryFilter::All => true,
            CategoryFilter::Uncategorized => item.category_id.trim().is_empty(),
            CategoryFilter::Id(id) => item.category_id == *id,
        }
    }

    pub fn category_name(&self, category_id: &str) -> Option<&str> {
        let id = category_id.trim();
        if id.is_empty() {
            return None;
        }
        self.categories
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.as_str())
    }

    pub fn reduce(&mut self, message: ModelMessage) -> ModelEffect {
        match message {
            ModelMessage::SwitchTab(tab) if tab == self.active_tab => ModelEffect::None,
            ModelMessage::SwitchTab(tab) if self.dirty => {
                self.pending_tab = Some(tab);
                ModelEffect::ConfirmDiscard
            }
            ModelMessage::SwitchTab(tab) => {
                self.active_tab = tab;
                ModelEffect::TabChanged(tab)
            }
            ModelMessage::ConfirmDiscard => {
                self.dirty = false;
                if let Some((index, item)) = self.deleted.take() {
                    self.matches.insert(index.min(self.matches.len()), item);
                }
                if let Some(tab) = self.pending_tab.take() {
                    self.active_tab = tab;
                    ModelEffect::TabChanged(tab)
                } else {
                    ModelEffect::None
                }
            }
            ModelMessage::CancelDiscard => {
                self.pending_tab = None;
                ModelEffect::None
            }
            ModelMessage::DraftChanged => {
                self.dirty = true;
                ModelEffect::None
            }
            ModelMessage::Saved => {
                self.dirty = false;
                self.deleted = None;
                ModelEffect::None
            }
            ModelMessage::FilterChanged(filter) => {
                self.filter = filter;
                ModelEffect::None
            }
            ModelMessage::CategoryFilterChanged(filter) => {
                self.category_filter = filter;
                ModelEffect::None
            }
            ModelMessage::Delete(id) => {
                if let Some(index) = self.matches.iter().position(|item| item.id == id) {
                    let item = self.matches.remove(index);
                    self.deleted = Some((index, item));
                    self.dirty = true;
                    ModelEffect::MatchesChanged
                } else {
                    ModelEffect::None
                }
            }
            ModelMessage::UndoDelete => {
                if let Some((index, item)) = self.deleted.take() {
                    self.matches.insert(index.min(self.matches.len()), item);
                    ModelEffect::MatchesChanged
                } else {
                    ModelEffect::None
                }
            }
        }
    }
}
