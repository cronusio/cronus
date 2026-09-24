//! Custom columns and saved board views as mapped extensions over the
//! canonical pipeline. The canonical `CardState` set stays the non-removable
//! backbone; custom structures extend it, never replace it.
//!
//! A custom column anchors to exactly one canonical state; every cross-cutting
//! consumer (archival, analytics, projections) reads the anchor, so a card in a
//! custom column *is* in its anchor state for them. A saved view is a filter over
//! the single card set — it holds no cards, and deleting it never touches cards.

use super::CardState;

/// A user-defined column that refines a canonical state (e.g. a `review` column
/// anchored to `running`). The anchor is part of the column's identity.
#[derive(Debug, Clone)]
pub struct CustomColumn {
    pub id: String,
    pub name: String,
    /// The canonical state this column maps to for all cross-cutting consumers.
    pub anchor: CardState,
}

/// An audit record of a column re-anchoring (a traceable, explicit change).
#[derive(Debug, Clone)]
pub struct AnchorAudit {
    pub column_id: String,
    pub from: CardState,
    pub to: CardState,
    pub actor: String,
    pub at: u64,
}

/// A saved board view — a filter/scope/grouping over the office's single card set.
/// Carries no state of its own and cannot hold cards the board of record does not.
#[derive(Debug, Clone)]
pub struct SavedView {
    pub id: String,
    pub name: String,
    /// The canonical states this view scopes to (empty = all).
    pub state_filter: Vec<CardState>,
}

impl SavedView {
    /// Whether a card in the given canonical state is included by this view.
    pub fn includes(&self, state: CardState) -> bool {
        self.state_filter.is_empty() || self.state_filter.contains(&state)
    }
}

/// Errors from custom-board operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomBoardError {
    /// A custom column id already exists.
    DuplicateColumn,
    /// No custom column with that id.
    UnknownColumn,
}

impl std::fmt::Display for CustomBoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            CustomBoardError::DuplicateColumn => "custom column id already exists",
            CustomBoardError::UnknownColumn => "unknown custom column",
        };
        f.write_str(s)
    }
}

impl std::error::Error for CustomBoardError {}

/// The custom-board extensions attached to one canonical board of record. Additive:
/// canonical columns are never stored here and never become removable.
#[derive(Debug, Default)]
pub struct BoardExtensions {
    columns: Vec<CustomColumn>,
    views: Vec<SavedView>,
    audit: Vec<AnchorAudit>,
}

impl BoardExtensions {
    pub fn new() -> Self {
        BoardExtensions::default()
    }

    /// Add a custom column with a mandatory canonical anchor.
    pub fn add_column(
        &mut self,
        id: &str,
        name: &str,
        anchor: CardState,
    ) -> Result<(), CustomBoardError> {
        if self.columns.iter().any(|c| c.id == id) {
            return Err(CustomBoardError::DuplicateColumn);
        }
        self.columns.push(CustomColumn {
            id: id.to_string(),
            name: name.to_string(),
            anchor,
        });
        Ok(())
    }

    /// Resolve a custom column to the canonical state cross-cutting consumers read.
    /// This is the guarantee: archival/analytics never see a non-canonical state.
    pub fn anchor_of(&self, column_id: &str) -> Option<CardState> {
        self.columns
            .iter()
            .find(|c| c.id == column_id)
            .map(|c| c.anchor)
    }

    /// Re-anchor a custom column to a different canonical state, appending an audit
    /// record (an explicit, traceable change).
    pub fn re_anchor(
        &mut self,
        column_id: &str,
        to: CardState,
        actor: &str,
        at: u64,
    ) -> Result<(), CustomBoardError> {
        let column = self
            .columns
            .iter_mut()
            .find(|c| c.id == column_id)
            .ok_or(CustomBoardError::UnknownColumn)?;
        let from = column.anchor;
        column.anchor = to;
        self.audit.push(AnchorAudit {
            column_id: column_id.to_string(),
            from,
            to,
            actor: actor.to_string(),
            at,
        });
        Ok(())
    }

    pub fn audit_log(&self) -> &[AnchorAudit] {
        &self.audit
    }

    /// Save a view (a filter over the single card set).
    pub fn add_view(&mut self, id: &str, name: &str, state_filter: Vec<CardState>) {
        self.views.push(SavedView {
            id: id.to_string(),
            name: name.to_string(),
            state_filter,
        });
    }

    pub fn view(&self, id: &str) -> Option<&SavedView> {
        self.views.iter().find(|v| v.id == id)
    }

    /// Remove a saved view. Returns whether one was removed. Views are disposable;
    /// this never touches any card.
    pub fn remove_view(&mut self, id: &str) -> bool {
        let before = self.views.len();
        self.views.retain(|v| v.id != id);
        self.views.len() != before
    }

    pub fn view_count(&self) -> usize {
        self.views.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_column_resolves_to_canonical_anchor() {
        // A custom column anchors to exactly one canonical state; consumers
        // read the anchor (a card in `review` is `running` for archival/analytics).
        let mut ext = BoardExtensions::new();
        ext.add_column("review", "In Review", CardState::Running)
            .unwrap();
        assert_eq!(ext.anchor_of("review"), Some(CardState::Running));
        assert_eq!(ext.anchor_of("nonexistent"), None);
    }

    #[test]
    fn duplicate_custom_column_rejected() {
        let mut ext = BoardExtensions::new();
        ext.add_column("review", "In Review", CardState::Running)
            .unwrap();
        assert_eq!(
            ext.add_column("review", "Dup", CardState::Todo),
            Err(CustomBoardError::DuplicateColumn)
        );
    }

    #[test]
    fn re_anchor_appends_audit_record() {
        // Re-anchoring is explicit and traceable.
        let mut ext = BoardExtensions::new();
        ext.add_column("icebox", "Icebox", CardState::Todo).unwrap();
        ext.re_anchor("icebox", CardState::Triage, "manager", 100)
            .unwrap();
        assert_eq!(ext.anchor_of("icebox"), Some(CardState::Triage));
        let audit = ext.audit_log();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].from, CardState::Todo);
        assert_eq!(audit[0].to, CardState::Triage);
        assert_eq!(audit[0].actor, "manager");
    }

    #[test]
    fn re_anchor_unknown_column_errors() {
        let mut ext = BoardExtensions::new();
        assert_eq!(
            ext.re_anchor("ghost", CardState::Done, "x", 1),
            Err(CustomBoardError::UnknownColumn)
        );
    }

    #[test]
    fn saved_view_is_a_filter_not_a_store() {
        // A view scopes the single card set; deleting it touches no cards.
        let mut ext = BoardExtensions::new();
        ext.add_view(
            "active",
            "Active work",
            vec![CardState::Running, CardState::Blocked],
        );
        let v = ext.view("active").unwrap();
        assert!(v.includes(CardState::Running));
        assert!(!v.includes(CardState::Todo));

        assert!(ext.remove_view("active"));
        assert_eq!(ext.view_count(), 0);
        assert!(!ext.remove_view("active")); // already gone
    }

    #[test]
    fn empty_filter_view_includes_all_states() {
        let mut ext = BoardExtensions::new();
        ext.add_view("all", "Everything", vec![]);
        let v = ext.view("all").unwrap();
        assert!(v.includes(CardState::Triage));
        assert!(v.includes(CardState::Done));
    }
}
