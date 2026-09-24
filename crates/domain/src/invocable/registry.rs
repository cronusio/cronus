//! The catalog half: one registration door for every action's descriptor,
//! the identity rules that keep a contribution from ever shadowing a core
//! name, the manifest-grant gate on registering at all, and the attribution
//! a contributed invocable cannot suppress. See [`super::dispatch`] for the
//! execution half.

use std::collections::{HashMap, HashSet};

use cronus_contract::{
    BINDER_NAME_MAX_LEN, INVOCABLE_GROUP_MAX_LEN, INVOCABLE_MAX_BINDERS, INVOCABLE_NAME_MAX_LEN,
    INVOCABLE_SUMMARY_MAX_LEN, Invocable, InvocableId, Resolved,
};

/// The reserved identity naming the core itself. No contribution may
/// register under this identity — enforced by pre-claiming it at
/// construction (see [`InvocableRegistry::new`]) rather than as a special
/// case in the registration logic, so the reservation and the general
/// collision rule are the same mechanism.
pub const CORE_IDENTITY: &str = "core";

/// The origin token the core itself registers under.
const CORE_SOURCE: &str = "core";

/// The general capability a contribution's manifest must declare before it
/// may register anything at all. A point requiring a further,
/// specific grant for a security-relevant invocable is a real refinement
/// this general gate does not yet model — deferred to whichever task wires
/// up real extension manifests, not silently dropped.
pub const CONTRIBUTE_GRANT: &str = "invocable:contribute";

/// Who is registering, and what physically distinguishes them.
///
/// `identity` is the qualifier a contribution's invocable ids are prefixed
/// with (human-chosen, e.g. an extension's own declared id). `source` is
/// assigned by whatever loads the registrant — never chosen by the identity
/// string alone — so two different origins that happen to pick the same
/// `identity` are still distinguishable, and the registry can tell "the same
/// extension registering more of its own invocables" apart from "two
/// different extensions colliding on one name". `grants` is what the
/// registrant's manifest declared — attaching to this registry at
/// all requires [`CONTRIBUTE_GRANT`] among them; the core registrant needs
/// none, being implicitly trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registrant {
    pub identity: String,
    pub source: String,
    pub grants: HashSet<String>,
}

impl Registrant {
    /// The one core registrant. Its identity is the reserved [`CORE_IDENTITY`].
    pub fn core() -> Self {
        Registrant {
            identity: CORE_IDENTITY.to_string(),
            source: CORE_SOURCE.to_string(),
            grants: HashSet::new(),
        }
    }

    /// A contribution's registrant: its own declared identity, and the
    /// origin token its loader assigned it. Carries no grants yet — chain
    /// [`Registrant::with_grant`] to declare what its manifest grants.
    pub fn extension(identity: impl Into<String>, source: impl Into<String>) -> Self {
        Registrant {
            identity: identity.into(),
            source: source.into(),
            grants: HashSet::new(),
        }
    }

    /// Declare one grant this registrant's manifest carries.
    pub fn with_grant(mut self, grant: impl Into<String>) -> Self {
        self.grants.insert(grant.into());
        self
    }
}

/// Why a descriptor field failed its bound. Refused outright — never
/// truncated, defaulted, or otherwise repaired — so this always names
/// exactly what was wrong rather than a declaration silently altered to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorFieldError {
    /// Empty, or whitespace-only.
    Empty,
    /// Longer than the field's declared bound.
    TooLong { max: usize },
    /// A collection field (currently only `binders`) exceeded its declared
    /// count bound.
    TooMany { max: usize },
}

/// Why a registration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationError {
    /// A descriptor field violated its bound — checked before
    /// anything about the registrant, since a malformed descriptor is
    /// invalid regardless of who is trying to register it.
    InvalidDescriptor {
        field: &'static str,
        problem: DescriptorFieldError,
    },
    /// The invocable's own qualified id names a different identity than the
    /// registrant claims to be.
    IdentityMismatch {
        declared: String,
        registrant: String,
    },
    /// This identity is already claimed by a different origin — including
    /// the reserved core identity, pre-claimed by [`CORE_SOURCE`] at
    /// construction, which is what makes claiming `"core"` as anyone else
    /// fail this same check rather than a separate one.
    IdentityCollision { identity: String },
    /// The registrant's manifest did not declare the grant this registry
    /// requires to register anything. Never checked for the core
    /// registrant, which needs no declared grant.
    MissingGrant { identity: String, grant: String },
    /// This exact qualified id is already registered.
    DuplicateId(InvocableId),
}

/// One bare-form shadowing event, returned as part of a successful
/// [`Registration`] when this registration changed who answers
/// [`InvocableRegistry::resolve_bare`] for a shared tail (the declared
/// *contribute* collision rule). `winner` now answers the bare-form lookup;
/// `loser` remains registered and reachable only by its qualified identity
/// until `winner` is disposed, at which point `loser` reclaims the slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shadow {
    pub winner: InvocableId,
    pub loser: InvocableId,
}

/// The effect that reverses one [`InvocableRegistry::register`] call:
/// disposing it removes exactly the descriptor it registered, and
/// its claim (winning or shadowed) on its tail's bare form — nothing else.
/// Consumed by value on disposal, so a handle can be spent only once.
#[derive(Debug)]
pub struct RegistrationHandle {
    id: InvocableId,
}

impl RegistrationHandle {
    /// Remove this exact registration from `registry`. If this was the
    /// tail's last remaining bare-form claimant, the tail's claimant list is
    /// dropped entirely rather than left behind as an empty entry — an
    /// empty list and an absent one must never be confused by a later
    /// lookup.
    pub fn dispose(self, registry: &mut InvocableRegistry) {
        registry.by_id.remove(&self.id);
        let tail = self.id.tail().to_string();
        if let Some(claimants) = registry.bare.get_mut(&tail) {
            claimants.retain(|claimed| claimed != &self.id);
            if claimants.is_empty() {
                registry.bare.remove(&tail);
            }
        }
        // Disposal is a mutation too — a verb belonging to a
        // deactivated extension must disappear from every live projection
        // at once, which requires the same announcement registration uses.
        registry.notify_observers();
    }
}

/// What a successful [`InvocableRegistry::register`] call returns: the
/// effect that reverses it, plus — only when this registration
/// event changed who holds the bare form for a shared tail — the shadowing
/// notice naming exactly who now wins and who does not. `shadow` is `None`
/// on the overwhelmingly common path where no other registrant shares this
/// tail at all.
#[derive(Debug)]
pub struct Registration {
    pub handle: RegistrationHandle,
    pub shadow: Option<Shadow>,
}

/// The bare-form winner among everyone currently registered under one tail:
/// the core entry if one is present, otherwise whoever registered earliest.
/// A pure function of the current claimant list, so the winner recomputes
/// correctly the moment a disposal changes that list — no cached "current
/// occupant" to keep in sync.
fn bare_winner(claimants: &[InvocableId]) -> &InvocableId {
    claimants
        .iter()
        .find(|id| id.qualifier() == CORE_IDENTITY)
        .unwrap_or(&claimants[0])
}

/// Bounds-check `invocable`'s own text and shape — refuses outright,
/// naming exactly which field and which bound was violated. `Binder` has no
/// `description` field, so only `name` is bounded on each; a validation
/// table entry that also names a binder description predates the field and
/// is a spec-wording defect, not a shape this crate has ever implemented.
fn validate_descriptor(invocable: &Invocable) -> Result<(), RegistrationError> {
    validate_text(invocable.name, "name", INVOCABLE_NAME_MAX_LEN)?;
    validate_text(invocable.summary, "summary", INVOCABLE_SUMMARY_MAX_LEN)?;
    validate_text(invocable.group, "group", INVOCABLE_GROUP_MAX_LEN)?;
    if invocable.binders.len() > INVOCABLE_MAX_BINDERS {
        return Err(RegistrationError::InvalidDescriptor {
            field: "binders",
            problem: DescriptorFieldError::TooMany {
                max: INVOCABLE_MAX_BINDERS,
            },
        });
    }
    for binder in &invocable.binders {
        validate_text(binder.name, "binder.name", BINDER_NAME_MAX_LEN)?;
    }
    Ok(())
}

fn validate_text(
    value: &'static str,
    field: &'static str,
    max_len: usize,
) -> Result<(), RegistrationError> {
    if value.trim().is_empty() {
        return Err(RegistrationError::InvalidDescriptor {
            field,
            problem: DescriptorFieldError::Empty,
        });
    }
    if value.len() > max_len {
        return Err(RegistrationError::InvalidDescriptor {
            field,
            problem: DescriptorFieldError::TooLong { max: max_len },
        });
    }
    Ok(())
}

/// One registration door; two lookup paths.
///
/// [`InvocableRegistry::resolve`] finds any registered invocable by its full
/// qualified id — always unambiguous. [`InvocableRegistry::resolve_bare`]
/// finds one by its unqualified tail alone, and resolves to whichever
/// registrant currently *wins* that tail under the declared collision rule:
/// the core always wins over any contribution, and among
/// contributions alone the earliest-registered wins. A losing registrant is
/// **shadowed, not displaced** — it stays registered and reachable by its
/// qualified id, and reclaims the bare form the moment the winner is
/// disposed (see [`Registration::shadow`]).
#[derive(Default)]
pub struct InvocableRegistry {
    by_id: HashMap<InvocableId, Invocable>,
    bare: HashMap<String, Vec<InvocableId>>,
    identity_sources: HashMap<String, String>,
    observers: Vec<Box<dyn RegistryObserver + Send + Sync>>,
}

// Manual, not derived: `Box<dyn RegistryObserver>` is neither `Debug` nor
// `Clone`, and requiring either of the trait would burden every observer
// implementation for a debugging convenience nothing in this codebase
// actually uses (checked: no caller formats or clones an `InvocableRegistry`
// today). Everything else is printed; the observer list is summarized by
// count instead of vanishing the derive entirely.
impl std::fmt::Debug for InvocableRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvocableRegistry")
            .field("by_id", &self.by_id)
            .field("bare", &self.bare)
            .field("identity_sources", &self.identity_sources)
            .field("observers", &self.observers.len())
            .finish()
    }
}

/// An observer of registry mutations: notified once a mutation is
/// already complete — never consulted before it, and never able to veto or
/// alter it. `on_change` takes `&InvocableRegistry`, not `&mut`, which is
/// what makes "cannot alter the mutation" a property the type system holds
/// rather than a convention observers are trusted to honor.
pub trait RegistryObserver {
    fn on_change(&self, registry: &InvocableRegistry);
}

impl InvocableRegistry {
    /// A registry with the reserved core identity already claimed — no
    /// registration step is required to establish it, and none can revoke
    /// it.
    pub fn new() -> Self {
        let mut identity_sources = HashMap::new();
        identity_sources.insert(CORE_IDENTITY.to_string(), CORE_SOURCE.to_string());
        InvocableRegistry {
            by_id: HashMap::new(),
            bare: HashMap::new(),
            identity_sources,
            observers: Vec::new(),
        }
    }

    /// Subscribe to every future mutation this registry makes — a
    /// successful `register` and a `RegistrationHandle::dispose` alike.
    pub fn observe(&mut self, observer: Box<dyn RegistryObserver + Send + Sync>) {
        self.observers.push(observer);
    }

    /// Notify every observer that a mutation just completed.
    /// Observer failures are contained **individually**: a panic caught
    /// here neither undoes the mutation, which already happened, nor
    /// prevents an observer registered after the panicking one from
    /// running. A registry that could not complete a registration because
    /// one UI's refresh callback misbehaved would be strictly worse than a
    /// UI left rendering a stale catalog — a real defect, but a contained
    /// one.
    fn notify_observers(&self) {
        for observer in &self.observers {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                observer.on_change(self);
            }));
        }
    }

    /// Register one invocable on behalf of `registrant`. The same function
    /// serves the core and every contribution alike — there is no
    /// separate, privileged registration path.
    ///
    /// A descriptor's own fields are validated first, before
    /// anything about the registrant is even consulted — a malformed
    /// descriptor is invalid regardless of who is trying to register it.
    /// `invocable` is moved in by value, never taken by reference, so the
    /// "normalized owned copy the registrant cannot afterwards reach" the contract
    /// asks for is a property of that move: there is no aliasing path back
    /// to what the registry now stores.
    pub fn register(
        &mut self,
        registrant: &Registrant,
        invocable: Invocable,
    ) -> Result<Registration, RegistrationError> {
        validate_descriptor(&invocable)?;

        let declared = invocable.id.qualifier();
        if declared != registrant.identity {
            return Err(RegistrationError::IdentityMismatch {
                declared: declared.to_string(),
                registrant: registrant.identity.clone(),
            });
        }

        if registrant.identity != CORE_IDENTITY && !registrant.grants.contains(CONTRIBUTE_GRANT) {
            return Err(RegistrationError::MissingGrant {
                identity: registrant.identity.clone(),
                grant: CONTRIBUTE_GRANT.to_string(),
            });
        }

        match self.identity_sources.get(&registrant.identity) {
            Some(existing_source) if existing_source != &registrant.source => {
                return Err(RegistrationError::IdentityCollision {
                    identity: registrant.identity.clone(),
                });
            }
            _ => {
                self.identity_sources
                    .insert(registrant.identity.clone(), registrant.source.clone());
            }
        }

        if self.by_id.contains_key(&invocable.id) {
            return Err(RegistrationError::DuplicateId(invocable.id.clone()));
        }

        // Every registrant contends for its tail's bare form — not core
        // alone — because a contribution must be able to *win* that form
        // when no core entry claims it, and reclaim it later if the core
        // entry claiming it is disposed (the declared collision rule).
        let tail = invocable.id.tail().to_string();
        let claimants = self.bare.entry(tail).or_default();
        let winner_before = (!claimants.is_empty()).then(|| bare_winner(claimants).clone());
        claimants.push(invocable.id.clone());
        let winner_after = bare_winner(claimants).clone();

        let shadow = match winner_before {
            None => None,
            Some(prev_winner) if prev_winner == winner_after => Some(Shadow {
                winner: winner_after,
                loser: invocable.id.clone(),
            }),
            Some(prev_winner) => Some(Shadow {
                winner: winner_after,
                loser: prev_winner,
            }),
        };

        let handle = RegistrationHandle {
            id: invocable.id.clone(),
        };
        self.by_id.insert(invocable.id.clone(), invocable);
        self.notify_observers();
        Ok(Registration { handle, shadow })
    }

    /// Look up by full qualified id — reaches core and contributed
    /// invocables alike. Returns [`Resolved`], not `Option`, so the
    /// "genuinely no such invocable" answer is a named domain fact
    /// a caller matches on, not a generic absence indistinguishable from
    /// any other `None`.
    pub fn resolve(&self, id: &InvocableId) -> Resolved<'_> {
        match self.by_id.get(id) {
            Some(invocable) => Resolved::Found(invocable),
            None => Resolved::Unknown,
        }
    }

    /// Look up by unqualified tail — resolves to whichever registrant
    /// currently wins that tail (see the struct-level doc), or `None` if no
    /// one has ever claimed it.
    pub fn resolve_bare(&self, tail: &str) -> Option<&Invocable> {
        let claimants = self.bare.get(tail)?;
        self.by_id.get(bare_winner(claimants))
    }

    /// Every currently registered descriptor, regardless of identity,
    /// locus, or stability — the one enumeration every generated
    /// projection (a parser, a catalog, a slash-command listing) builds
    /// from. Callers filter by whatever they actually need (a shipped-only
    /// listing, one locus); this method stays a plain enumeration rather
    /// than encoding any one caller's policy.
    pub fn all(&self) -> impl Iterator<Item = &Invocable> {
        self.by_id.values()
    }
}

/// The core-drawn attribution for a contributed invocable, or `None` for a
/// core one. Always derived from the id's own qualifier — which
/// [`InvocableRegistry::register`] already validated against the actual
/// registrant before ever accepting it — so nothing the invocable's *other*
/// fields say (`name`, `summary`) can alter or suppress it: the projection
/// draws this independently of whatever the contribution would prefer
/// displayed.
pub fn attribution(invocable: &Invocable) -> Option<&str> {
    let qualifier = invocable.id.qualifier();
    if qualifier == CORE_IDENTITY {
        None
    } else {
        Some(qualifier)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use cronus_contract::{Locus, Stability};

    fn sample_invocable(id: &str) -> Invocable {
        Invocable {
            id: InvocableId::new(id).expect("well-formed invocable id"),
            name: "Sample",
            summary: "A sample invocable for registry tests.",
            group: "test",
            locus: Locus::Semantic,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        }
    }

    fn granted(identity: &str, source: &str) -> Registrant {
        Registrant::extension(identity, source).with_grant(CONTRIBUTE_GRANT)
    }

    #[test]
    fn a_contribution_cannot_claim_the_reserved_core_identity() {
        let mut registry = InvocableRegistry::new();
        // Even granted, an impostor claiming the reserved identity is
        // refused by the identity-collision check, not by the grant gate.
        let impostor = granted(CORE_IDENTITY, "malicious-ext");
        let err = registry
            .register(&impostor, sample_invocable("core:board.list"))
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::IdentityCollision {
                identity: CORE_IDENTITY.to_string()
            }
        );
    }

    #[test]
    fn a_bare_verb_collision_resolves_to_the_core_registrant_regardless_of_order() {
        let mut registry = InvocableRegistry::new();
        // The contribution registers FIRST, to prove the outcome does not
        // depend on registration order.
        registry
            .register(
                &granted("myext", "src-1"),
                sample_invocable("myext:board.list"),
            )
            .unwrap();
        registry
            .register(&Registrant::core(), sample_invocable("core:board.list"))
            .unwrap();

        let resolved = registry.resolve_bare("board.list").expect("bare lookup");
        assert_eq!(resolved.id.as_str(), "core:board.list");

        // The contribution stays reachable by its own qualified id.
        assert!(
            registry
                .resolve(&InvocableId::new("myext:board.list").expect("well-formed invocable id"))
                .is_found()
        );
    }

    #[test]
    fn two_sources_claiming_one_identity_the_later_is_refused() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&granted("myext", "src-1"), sample_invocable("myext:one"))
            .unwrap();

        let err = registry
            .register(&granted("myext", "src-2"), sample_invocable("myext:two"))
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::IdentityCollision {
                identity: "myext".to_string()
            }
        );

        // The SAME source registering more under its own identity is fine.
        assert!(
            registry
                .register(&granted("myext", "src-1"), sample_invocable("myext:three"))
                .is_ok()
        );
    }

    #[test]
    fn declared_qualifier_must_match_the_registrants_own_identity() {
        let mut registry = InvocableRegistry::new();
        let err = registry
            .register(
                &granted("myext", "src-1"),
                sample_invocable("otherext:verb"),
            )
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::IdentityMismatch {
                declared: "otherext".to_string(),
                registrant: "myext".to_string(),
            }
        );
    }

    #[test]
    fn duplicate_qualified_id_is_refused() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), sample_invocable("core:board.list"))
            .unwrap();
        let err = registry
            .register(&Registrant::core(), sample_invocable("core:board.list"))
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::DuplicateId(
                InvocableId::new("core:board.list").expect("well-formed invocable id")
            )
        );
    }

    #[test]
    fn registering_without_the_declared_contribute_grant_is_refused() {
        let mut registry = InvocableRegistry::new();
        let ungranted = Registrant::extension("myext", "src-1");
        let err = registry
            .register(&ungranted, sample_invocable("myext:verb"))
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::MissingGrant {
                identity: "myext".to_string(),
                grant: CONTRIBUTE_GRANT.to_string(),
            }
        );

        // The same registration, with the grant declared, succeeds.
        assert!(
            registry
                .register(&granted("myext", "src-1"), sample_invocable("myext:verb"))
                .is_ok()
        );
    }

    #[test]
    fn the_core_registrant_needs_no_declared_grant() {
        let mut registry = InvocableRegistry::new();
        assert!(
            registry
                .register(&Registrant::core(), sample_invocable("core:board.list"))
                .is_ok()
        );
    }

    #[test]
    fn all_enumerates_every_registration_regardless_of_registrant() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), sample_invocable("core:board.list"))
            .unwrap();
        registry
            .register(&granted("myext", "src-1"), sample_invocable("myext:one"))
            .unwrap();

        let mut ids: Vec<&str> = registry
            .all()
            .map(|invocable| invocable.id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["core:board.list", "myext:one"]);
    }

    #[test]
    fn attribution_is_derived_from_the_validated_identity_never_from_display_fields() {
        let mut deceptive = sample_invocable("myext:settings");
        // A contribution trying to look like a core feature through the
        // fields it does control.
        deceptive.name = "Core Settings";
        deceptive.summary = "Built-in configuration";
        assert_eq!(attribution(&deceptive), Some("myext"));

        let core_invocable = sample_invocable("core:board.list");
        assert_eq!(attribution(&core_invocable), None);
    }

    #[test]
    fn a_shadowed_contribution_returns_to_the_bare_form_once_the_shadowing_entry_is_disposed() {
        let mut registry = InvocableRegistry::new();
        // The contribution registers FIRST and is briefly the sole
        // claimant — resolve_bare would answer it if asked right now.
        registry
            .register(
                &granted("myext", "src-1"),
                sample_invocable("myext:board.list"),
            )
            .unwrap();

        let core_registration = registry
            .register(&Registrant::core(), sample_invocable("core:board.list"))
            .unwrap();

        // The core's arrival shadows the contribution — reported by name on
        // the registration that caused it, not left for the caller to infer.
        assert_eq!(
            core_registration.shadow,
            Some(Shadow {
                winner: InvocableId::new("core:board.list").expect("well-formed invocable id"),
                loser: InvocableId::new("myext:board.list").expect("well-formed invocable id"),
            })
        );
        assert_eq!(
            registry
                .resolve_bare("board.list")
                .expect("core wins the bare form")
                .id
                .as_str(),
            "core:board.list"
        );
        assert!(
            registry
                .resolve(&InvocableId::new("myext:board.list").expect("well-formed invocable id"))
                .is_found(),
            "the shadowed contribution stays reachable by its qualified id throughout"
        );

        // Disposing the shadowing (core) registration lets the contribution
        // reclaim the bare form it would otherwise have held alone.
        core_registration.handle.dispose(&mut registry);

        assert_eq!(
            registry
                .resolve_bare("board.list")
                .expect("the contribution reclaims the bare form")
                .id
                .as_str(),
            "myext:board.list"
        );
    }

    struct PanickingObserver;
    impl RegistryObserver for PanickingObserver {
        fn on_change(&self, _registry: &InvocableRegistry) {
            panic!("this observer deliberately misbehaves");
        }
    }

    struct CountingObserver(Arc<AtomicUsize>);
    impl RegistryObserver for CountingObserver {
        fn on_change(&self, _registry: &InvocableRegistry) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_panicking_observer_does_not_prevent_registration_or_starve_a_later_observer() {
        let mut registry = InvocableRegistry::new();
        let calls = Arc::new(AtomicUsize::new(0));
        registry.observe(Box::new(PanickingObserver));
        registry.observe(Box::new(CountingObserver(Arc::clone(&calls))));

        let result = registry.register(&Registrant::core(), sample_invocable("core:board.list"));

        assert!(
            result.is_ok(),
            "the mutation completes despite a panicking observer"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the observer registered after the panicking one still ran"
        );
    }

    #[test]
    fn disposal_also_announces_a_change() {
        let mut registry = InvocableRegistry::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let registration = registry
            .register(&Registrant::core(), sample_invocable("core:board.list"))
            .unwrap();

        // Only observe from here — the registration above already fired
        // once, and this test is specifically about disposal's own
        // announcement, not registration's.
        registry.observe(Box::new(CountingObserver(Arc::clone(&calls))));
        registration.handle.dispose(&mut registry);

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_empty_descriptor_field_is_refused_and_names_the_field() {
        let mut invocable = sample_invocable("core:board.list");
        invocable.name = "";
        let err = InvocableRegistry::new()
            .register(&Registrant::core(), invocable)
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::InvalidDescriptor {
                field: "name",
                problem: DescriptorFieldError::Empty,
            }
        );
    }

    #[test]
    fn a_whitespace_only_descriptor_field_is_refused_as_empty() {
        let mut invocable = sample_invocable("core:board.list");
        invocable.summary = "   ";
        let err = InvocableRegistry::new()
            .register(&Registrant::core(), invocable)
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::InvalidDescriptor {
                field: "summary",
                problem: DescriptorFieldError::Empty,
            }
        );
    }

    #[test]
    fn an_over_long_descriptor_field_is_refused_and_names_its_bound() {
        // 41 chars — one over BINDER_NAME_MAX_LEN (40) — inside a leaked
        // Box so the invocable's own binder field, `&'static str`, can hold
        // a string this test built at runtime.
        let long_name: &'static str =
            Box::leak("x".repeat(BINDER_NAME_MAX_LEN + 1).into_boxed_str());
        let mut invocable = sample_invocable("core:board.add");
        invocable.binders = vec![cronus_contract::Binder {
            name: long_name,
            kind: cronus_contract::BinderKind::Text,
            optional: false,
        }];
        let err = InvocableRegistry::new()
            .register(&Registrant::core(), invocable)
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::InvalidDescriptor {
                field: "binder.name",
                problem: DescriptorFieldError::TooLong {
                    max: BINDER_NAME_MAX_LEN
                },
            }
        );
    }

    #[test]
    fn too_many_binders_is_refused_and_names_the_bound() {
        let binder = cronus_contract::Binder {
            name: "id",
            kind: cronus_contract::BinderKind::Text,
            optional: false,
        };
        let mut invocable = sample_invocable("core:board.add");
        invocable.binders = vec![binder; INVOCABLE_MAX_BINDERS + 1];
        let err = InvocableRegistry::new()
            .register(&Registrant::core(), invocable)
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::InvalidDescriptor {
                field: "binders",
                problem: DescriptorFieldError::TooMany {
                    max: INVOCABLE_MAX_BINDERS
                },
            }
        );
    }

    #[test]
    fn a_malformed_descriptor_is_refused_before_any_registrant_check_runs() {
        // The registrant/identity mismatch below would ALSO fail this
        // registration — proving descriptor validation runs first, not
        // merely that some error is returned.
        let mut invocable = sample_invocable("otherext:verb");
        invocable.name = "";
        let err = InvocableRegistry::new()
            .register(&granted("myext", "src-1"), invocable)
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::InvalidDescriptor {
                field: "name",
                problem: DescriptorFieldError::Empty,
            }
        );
    }
}
