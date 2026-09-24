//! What a surface exposes to the corpus — expressed only in
//! ports-tier types, so a surface whose entire mechanism is a remote IPC
//! bridge (the desktop shell, across its process boundary) can register a
//! projection exactly as directly as one holding a live registry and
//! dispatcher in-process (the CLI, the terminal UI). Nothing here names
//! `InvocableRegistry` or `Dispatcher` — a surface's test target builds
//! those (or their IPC-bridged equivalent) itself and answers through this
//! trait.

use cronus_contract::{ArgValues, Binder, Dispatched, Invocable, InvocableId};

/// One surface's real command projection, as the corpus needs to see it.
/// Each method answers from the surface's **actual** projection — its live
/// registry, or a real round trip across its IPC seam — never a restatement
/// of the fixture data itself; a projection that merely echoed the fixture
/// back would prove the fixture exists, not that the surface reaches it.
pub trait SurfaceProjection {
    /// Every invocable this surface currently exposes on its default,
    /// shipped listing (help, completion, discovery) — already filtered by
    /// whatever this surface has declared it deliberately does not expose.
    /// Compared against the corpus's canonical shipped set, minus
    /// the caller's own declared exclusions (the surface-set family).
    fn exposed(&self) -> Vec<Invocable>;

    /// The argument schema this surface currently advertises for `id` —
    /// from its real help text, completion metadata, or generated parser,
    /// not from re-reading the canonical descriptor. `None` means this
    /// surface does not know the id at all, which is itself a surface-set
    /// finding, not a schema one.
    fn advertised_binders(&self, id: &InvocableId) -> Option<Vec<Binder>>;

    /// Drive one invocation through this surface's real dispatch path —
    /// the in-process `Dispatcher::dispatch` for the CLI and TUI, or a
    /// genuine serialize/deserialize round trip across the desktop's IPC
    /// seam — and return what it produced.
    fn invoke(&self, id: &InvocableId, args: &ArgValues) -> Dispatched;
}
