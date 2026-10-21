//! The gate in front of an installation change that recomposes the office.
//!
//! A change of that class alters what running work is built from, so it is
//! applied only while nothing is running and while nothing can begin. The
//! operating system decides both at once through the work lock
//! ([`crate::endpoint::worklock`]): every producer of work holds it shared, the
//! change takes it exclusive, and neither can take it over the other. A change
//! refused here is refused because the lock is held — never on an assumption
//! that the office is idle — and the refusal then asks the engine, when there
//! is one, *what* is running, so the person is told rather than just refused.

use std::path::Path;

use crate::endpoint::ledger::{self, WorkItem, describe_running};
use crate::endpoint::worklock::{self, Contended, Exclusive};

/// Why the change may not be applied now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Work is running that the change would alter, or another change is being
    /// applied. The sentence says what it can.
    Busy(String),
    /// The work lock could not be asked, so nothing is assumed.
    Unknown(String),
}

impl Refused {
    /// The reason, in a person's words.
    pub fn reason(&self) -> &str {
        match self {
            Refused::Busy(reason) | Refused::Unknown(reason) => reason,
        }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

/// A change being applied: while this exists no new work begins over the state
/// root, in any process. Dropping it ends the claim.
pub struct Applying {
    _exclusive: Exclusive,
}

/// What a change alters, and so whose work it must wait for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The workspace's own state: its archetype, its stores, its restore.
    Workspace,
    /// The machine's: agent definitions, the extension registry, the
    /// developer-office admission. Work in every workspace is built from them.
    Machine,
}

/// The scope of the verb whose id tail is `tail` (`registry.enable`).
pub fn scope_of(tail: &str) -> Scope {
    if ["registry.", "ext.", "dev."]
        .iter()
        .any(|prefix| tail.starts_with(prefix))
    {
        Scope::Machine
    } else {
        Scope::Workspace
    }
}

/// Begin applying the change the verb with id tail `tail` makes, over the
/// state root whose work it must wait for.
pub fn begin_for(tail: &str) -> Result<Applying, Refused> {
    let root = match scope_of(tail) {
        Scope::Workspace => crate::paths::resolve_workspace_root(),
        Scope::Machine => crate::endpoint::machine_root(),
    };
    begin(&root)
}

/// Whether the change the verb with id tail `tail` makes could be applied
/// right now — the question a surface asks *before* it asks the person, so
/// nobody is invited to agree to something that can only be refused. The answer
/// is a moment's: the handler asks again when it applies, and the answer it
/// gets then is the one that counts.
pub fn check_for(tail: &str) -> Result<(), Refused> {
    begin_for(tail).map(drop)
}

/// Begin applying a recomposing change over `root`.
pub fn begin(root: &Path) -> Result<Applying, Refused> {
    match worklock::exclusive(root) {
        Ok(exclusive) => Ok(Applying {
            _exclusive: exclusive,
        }),
        Err(Contended::Held) => Err(Refused::Busy(what_is_running(root))),
        Err(Contended::Io(err)) => Err(Refused::Unknown(format!(
            "this cannot be told whether work is running, so the change is not applied: {err}"
        ))),
    }
}

/// What a refusal can say about the work that stopped the change: the names
/// this process knows, and the engine's when one is reachable. The lock says
/// *that* work is running; only a ledger can say what, and a process the engine
/// does not know of (a standalone command) has none.
fn what_is_running(root: &Path) -> String {
    let mut items: Vec<WorkItem> = ledger::global().snapshot();
    if let Some(engine) = crate::endpoint::reach_engine(root)
        && let Ok(named) = engine.work()
    {
        items.extend(named);
    }
    if items.is_empty() {
        "work is running in another process, or another installation change is being applied, \
         that this change would alter"
            .to_string()
    } else {
        describe_running(&items)
    }
}
