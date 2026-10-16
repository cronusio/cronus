//! This surface's own actions — pane focus and quitting the session —
//! registered as `ClientLocal` invocables through the same door a core or
//! contributed verb uses.
//!
//! [`PaneAction`] is the single source of truth for what these actions are:
//! [`register`] iterates every variant to build the registry entry, and
//! `app.rs`'s key handler resolves a key press to the same variant before
//! dispatching — so the two can never name a different set. What is
//! deliberately NOT here is which key triggers which action: key bindings
//! are terminal-surface presentation and stay local to `app.rs` (the
//! do-not-unify record already treats input mechanics as legitimately
//! per-surface); what must not stay local is the action's own declaration.
//!
//! Each handler is a stateless acknowledgment — `Outcome::Value(Empty)`,
//! nothing more. The actual view-state mutation (which panel gains focus,
//! whether the loop should exit) happens in `app.rs`, which already knows
//! which [`PaneAction`] a key resolved to before it ever called dispatch.
//! That is a legitimate, disclosed division for a `ClientLocal` action: per
//! the registry spec, "the same identity may be implemented per surface" —
//! unlike a `Semantic` verb, nothing here delegates to the core. What a real
//! dispatch through the registry proves is that the action is reachable
//! through the one shared door and not through a second, private path: an
//! action that failed to register, or was never attached, resolves to
//! `Dispatched::Unknown` here exactly as a genuinely unbound verb would
//! anywhere else in this crate, and `app.rs` applies no effect in that case.

use std::sync::Arc;

use cronus_contract::{Invocable, InvocableId, Locus, Outcome, OutcomeValue, Stability};
use cronus_core::invocable::{Dispatcher, InvocableRegistry, Registrant};

use crate::view::Focus;

/// One action that exists only on this surface. See the module doc for why
/// this enum, rather than a table in `app.rs`, is the source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneAction {
    /// Advance keyboard focus to the next panel in tab order.
    FocusNext,
    /// Move keyboard focus to the previous panel in tab order.
    FocusPrev,
    /// Move keyboard focus straight to the command bar from any panel.
    FocusCommandBar,
    /// Exit the terminal session.
    Quit,
    /// Move keyboard focus to the Kanban board panel.
    FocusBoard,
    /// Move keyboard focus to the Office panel.
    FocusOffice,
    /// Move keyboard focus to the Sessions panel.
    FocusSessions,
    /// Show the command listing.
    Help,
    /// Open the Chat screen with the command bar ready for a message.
    FocusChat,
}

impl PaneAction {
    /// Every action this surface declares — iterated by [`register`], and
    /// available for a test to enumerate what "reachable through the
    /// registry" is supposed to mean.
    pub const ALL: [PaneAction; 9] = [
        PaneAction::FocusNext,
        PaneAction::FocusPrev,
        PaneAction::FocusCommandBar,
        PaneAction::Quit,
        PaneAction::FocusBoard,
        PaneAction::FocusOffice,
        PaneAction::FocusSessions,
        PaneAction::Help,
        PaneAction::FocusChat,
    ];

    pub(crate) fn verb(self) -> &'static str {
        match self {
            PaneAction::FocusNext => "focus-next",
            PaneAction::FocusPrev => "focus-prev",
            PaneAction::FocusCommandBar => "focus-command",
            PaneAction::Quit => "quit",
            PaneAction::FocusBoard => "focus-board",
            PaneAction::FocusOffice => "focus-office",
            PaneAction::FocusSessions => "focus-sessions",
            PaneAction::Help => "help",
            PaneAction::FocusChat => "focus-chat",
        }
    }

    fn name(self) -> &'static str {
        match self {
            PaneAction::FocusNext => "Focus next",
            PaneAction::FocusPrev => "Focus previous",
            PaneAction::FocusCommandBar => "Focus command bar",
            PaneAction::Quit => "Quit",
            PaneAction::FocusBoard => "Focus board",
            PaneAction::FocusOffice => "Focus office",
            PaneAction::FocusSessions => "Focus sessions",
            PaneAction::Help => "Help",
            PaneAction::FocusChat => "Focus chat",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            PaneAction::FocusNext => "Move keyboard focus to the next panel.",
            PaneAction::FocusPrev => "Move keyboard focus to the previous panel.",
            PaneAction::FocusCommandBar => "Move keyboard focus straight to the command bar.",
            PaneAction::Quit => "Exit the terminal session.",
            PaneAction::FocusBoard => "Move keyboard focus to the Kanban board panel.",
            PaneAction::FocusOffice => "Move keyboard focus to the Office panel.",
            PaneAction::FocusSessions => "Move keyboard focus to the Sessions panel.",
            PaneAction::Help => "Show the command listing.",
            PaneAction::FocusChat => "Open the Chat screen.",
        }
    }

    /// This action's qualified identity in the shared registry — the same
    /// identity a key binding in `app.rs` resolves to before dispatching.
    pub fn id(self) -> InvocableId {
        InvocableId::new(format!("core:pane.{}", self.verb()))
            .expect("a literal pane-action identity must be well-formed — a bug if it isn't")
    }

    /// The action a slash command's own `group`/sub-verb name, if any —
    /// checked against this exact enum rather than by re-deriving the
    /// `core:pane.{verb}` identity string, so the command bar's dispatch
    /// path can recognize "this submitted line names one of my own actions"
    /// without constructing an [`InvocableId`] just to compare it.
    pub(crate) fn from_slash(group: &str, verb: &str) -> Option<Self> {
        if group != "pane" {
            return None;
        }
        Self::ALL.into_iter().find(|action| action.verb() == verb)
    }
}

/// What the function-key bar offers on key `n` (1–10): the label it shows
/// and the action the key runs. A key with no entry is drawn as a bare number
/// and does nothing — a slot is never labelled with something this surface
/// cannot do.
pub fn fkey(n: u8) -> Option<(&'static str, PaneAction)> {
    match n {
        1 => Some(("Help", PaneAction::Help)),
        3 => Some(("Chat", PaneAction::FocusChat)),
        4 => Some(("Sess", PaneAction::FocusSessions)),
        6 => Some(("Office", PaneAction::FocusOffice)),
        7 => Some(("Board", PaneAction::FocusBoard)),
        10 => Some(("Quit", PaneAction::Quit)),
        _ => None,
    }
}

/// The function key whose action leads to `focus`, if the bar has one — the
/// slot drawn as active while that panel holds focus.
pub fn fkey_for_focus(focus: Focus) -> Option<u8> {
    (1..=10u8).find(|n| {
        matches!(
            (fkey(*n), focus),
            (Some((_, PaneAction::FocusBoard)), Focus::Board)
                | (Some((_, PaneAction::FocusOffice)), Focus::Office)
                | (Some((_, PaneAction::FocusSessions)), Focus::Sessions)
        )
    })
}

/// Register every [`PaneAction`] as a `ClientLocal` invocable, group
/// `"pane"`, through [`InvocableRegistry::register`] — the identical door
/// `crates/core/src/invocable_bootstrap` uses for a core verb.
pub fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    for action in PaneAction::ALL {
        let id = action.id();
        let invocable = Invocable {
            id: id.clone(),
            name: action.name(),
            summary: action.summary(),
            group: "pane",
            locus: Locus::ClientLocal,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        };
        registry
            .register(&Registrant::core(), invocable)
            .expect("a pane action registers cleanly at startup — a duplicate id is a bug here");
        dispatcher.attach(id, Arc::new(|_args| Outcome::Value(OutcomeValue::Empty)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::Resolved;

    fn built() -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        register(&mut registry, &mut dispatcher);
        (registry, dispatcher)
    }

    /// Every declared action is a real, resolvable `ClientLocal` descriptor
    /// — not merely a key binding a reader has to notice in `app.rs`.
    #[test]
    fn every_pane_action_resolves_as_a_shipped_client_local_descriptor() {
        let (registry, _dispatcher) = built();
        for action in PaneAction::ALL {
            match registry.resolve(&action.id()) {
                Resolved::Found(descriptor) => {
                    assert_eq!(descriptor.locus, Locus::ClientLocal);
                    assert_eq!(descriptor.stability, Stability::Shipped);
                    assert_eq!(descriptor.group, "pane");
                }
                Resolved::Unknown => panic!("{:?} must resolve after register()", action),
            }
        }
    }

    /// Dispatch, not a second path, is what makes an action run.
    #[test]
    fn dispatching_a_registered_pane_action_runs_its_handler() {
        let (registry, dispatcher) = built();
        let invocation = cronus_contract::Invocation {
            id: PaneAction::FocusNext.id(),
            args: cronus_contract::ArgValues::new(),
            caller: cronus_contract::Surface::Tui,
        };
        match dispatcher.dispatch(&registry, &invocation) {
            cronus_contract::Dispatched::Ran(Outcome::Value(OutcomeValue::Empty)) => {}
            other => panic!("expected a real, resolved run, got {other:?}"),
        }
    }

    /// An action nobody registered resolves to `Unknown` — the same answer a
    /// genuinely unbound verb gets, proving there is no private fallback
    /// that would make the actual effect happen even without a real
    /// registration.
    #[test]
    fn an_unregistered_pane_action_resolves_to_unknown() {
        let registry = InvocableRegistry::new();
        let dispatcher = Dispatcher::new();
        let invocation = cronus_contract::Invocation {
            id: PaneAction::Quit.id(),
            args: cronus_contract::ArgValues::new(),
            caller: cronus_contract::Surface::Tui,
        };
        assert!(matches!(
            dispatcher.dispatch(&registry, &invocation),
            cronus_contract::Dispatched::Unknown
        ));
    }

    /// `from_slash` recognizes every declared action by its own group/verb —
    /// the same pair the command bar splits a submitted line into — so the
    /// app's command-bar dispatch path can find the right variant without
    /// re-deriving an [`InvocableId`] just to compare it.
    #[test]
    fn from_slash_recognizes_every_declared_action_by_its_own_verb() {
        for action in PaneAction::ALL {
            assert_eq!(PaneAction::from_slash("pane", action.verb()), Some(action));
        }
    }

    #[test]
    fn from_slash_rejects_a_different_group_or_an_unknown_verb() {
        assert_eq!(PaneAction::from_slash("board", "focus-next"), None);
        assert_eq!(PaneAction::from_slash("pane", "bogus"), None);
    }

    #[test]
    fn the_function_key_bar_only_names_declared_actions_and_each_focus_key_leads_back() {
        for n in 1..=10u8 {
            if let Some((label, action)) = fkey(n) {
                assert!(!label.is_empty());
                assert!(
                    PaneAction::ALL.contains(&action),
                    "F{n} runs an undeclared action"
                );
            }
        }
        for focus in [Focus::Board, Focus::Office, Focus::Sessions] {
            let n = fkey_for_focus(focus).expect("a panel with a key");
            let (_, action) = fkey(n).expect("the key is bound");
            let reached = match action {
                PaneAction::FocusBoard => Focus::Board,
                PaneAction::FocusOffice => Focus::Office,
                PaneAction::FocusSessions => Focus::Sessions,
                other => panic!("F{n} should focus a panel, runs {other:?}"),
            };
            assert_eq!(reached, focus);
        }
        assert_eq!(
            fkey_for_focus(Focus::Status),
            None,
            "the status panel has no key"
        );
    }
}
