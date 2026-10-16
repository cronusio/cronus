//! The Chat screen's own commands — choosing the model and starting a fresh
//! conversation — registered as `ClientLocal` invocables through the same door
//! as every other action, so they are discoverable (`/chat`) and never a
//! private table.
//!
//! Like the pane actions, the registered handler is a stateless
//! acknowledgment: what the action *does* — the view's chosen model, the
//! conversation it is in — is view state, applied by `app.rs`.

use std::sync::Arc;

use cronus_contract::{
    Binder, BinderKind, Invocable, InvocableId, Locus, Outcome, OutcomeValue, Stability,
};
use cronus_core::invocable::{Dispatcher, InvocableRegistry, Registrant};

/// One action of the Chat screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAction {
    /// Choose the model the conversation talks to.
    Model,
    /// Leave the current conversation and start a new one with the next message.
    New,
}

impl ChatAction {
    /// Every action this screen declares.
    pub const ALL: [ChatAction; 2] = [ChatAction::Model, ChatAction::New];

    fn verb(self) -> &'static str {
        match self {
            ChatAction::Model => "model",
            ChatAction::New => "new",
        }
    }

    fn name(self) -> &'static str {
        match self {
            ChatAction::Model => "Chat model",
            ChatAction::New => "Chat new",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            ChatAction::Model => "Choose the model the chat talks to.",
            ChatAction::New => "Start a new conversation with the next message.",
        }
    }

    fn binders(self) -> Vec<Binder> {
        match self {
            ChatAction::Model => vec![Binder {
                name: "name",
                kind: BinderKind::Text,
                optional: false,
            }],
            ChatAction::New => Vec::new(),
        }
    }

    /// This action's identity in the shared registry.
    pub fn id(self) -> InvocableId {
        InvocableId::new(format!("core:chat.{}", self.verb()))
            .expect("a literal chat-action identity must be well-formed — a bug if it isn't")
    }

    /// The action a slash command's group and first argument name, if any.
    pub(crate) fn from_slash(group: &str, verb: &str) -> Option<Self> {
        if group != "chat" {
            return None;
        }
        Self::ALL.into_iter().find(|action| action.verb() == verb)
    }
}

/// Register every [`ChatAction`] as a `ClientLocal` invocable of group `chat`.
pub fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    for action in ChatAction::ALL {
        let id = action.id();
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: id.clone(),
                    name: action.name(),
                    summary: action.summary(),
                    group: "chat",
                    locus: Locus::ClientLocal,
                    binders: action.binders(),
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("a chat action registers cleanly at startup — a duplicate id is a bug here");
        dispatcher.attach(id, Arc::new(|_args| Outcome::Value(OutcomeValue::Empty)));
    }
}

#[cfg(test)]
mod tests {
    use cronus_contract::{Dispatched, Invocation, Resolved, Surface};

    use super::*;

    fn built() -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        register(&mut registry, &mut dispatcher);
        (registry, dispatcher)
    }

    #[test]
    fn every_chat_action_is_a_registered_client_local_descriptor_with_a_handler() {
        let (registry, dispatcher) = built();
        for action in ChatAction::ALL {
            match registry.resolve(&action.id()) {
                Resolved::Found(descriptor) => {
                    assert_eq!(descriptor.locus, Locus::ClientLocal);
                    assert_eq!(descriptor.group, "chat");
                }
                Resolved::Unknown => panic!("{:?} did not register", action),
            }
            assert!(dispatcher.has_handler(&action.id()));
        }
    }

    #[test]
    fn choosing_a_model_requires_naming_one() {
        let (registry, dispatcher) = built();
        let invocation = Invocation {
            id: ChatAction::Model.id(),
            args: cronus_contract::ArgValues::new(),
            caller: Surface::Tui,
        };
        match dispatcher.dispatch(&registry, &invocation) {
            Dispatched::Ran(Outcome::Rejected(rejection)) => assert_eq!(rejection.binder, "name"),
            other => panic!("expected a rejection naming the binder, got {other:?}"),
        }
    }

    #[test]
    fn a_slash_command_recognizes_only_its_own_actions() {
        assert_eq!(
            ChatAction::from_slash("chat", "model"),
            Some(ChatAction::Model)
        );
        assert_eq!(ChatAction::from_slash("chat", "new"), Some(ChatAction::New));
        assert_eq!(ChatAction::from_slash("chat", "other"), None);
        assert_eq!(ChatAction::from_slash("pane", "model"), None);
    }
}
