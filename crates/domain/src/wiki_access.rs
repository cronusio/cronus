//! Access-gated project-wiki reads.
//!
//! The client never touches [`cronus_contract::WikiReadSurface`] directly on a
//! shared office — it goes through [`GatedWiki`], which runs the uniform
//! `access-grants` check (`has_access(Wiki, office_id, Read)`) **before every
//! read** and only then delegates. Read access thus follows the office's
//! sharing posture: a private office (owner-only, no grants) is readable by its
//! owner alone; a shared office is readable by exactly the principals granted
//! `Read` (or `Write`, which implies `Read`).
//!
//! The gate composes two seams without coupling them: the read surface is the
//! ports-tier [`WikiReadSurface`]; the grant algebra is this crate's
//! [`GrantStore`]. Neither knows about the other — `GatedWiki` is the single
//! place they meet, so the enforcement lives in one auditable spot rather than
//! being sprinkled through the read paths (the uniform-primitive intent).

use cronus_contract::{WikiChangelogEntry, WikiPage, WikiReadSurface};

use crate::resource_sharing::{GrantStore, Permission, ResourceKind};

/// The caller's identity for a wiki read. `is_owner` short-circuits the
/// grant lookup (the owner always reads their own office's wiki);
/// `groups` are the caller's group memberships, pre-resolved once per request
/// as the grant model expects.
#[derive(Debug, Clone)]
pub struct WikiPrincipal {
    pub user_id: String,
    pub is_owner: bool,
    pub groups: Vec<String>,
}

impl WikiPrincipal {
    /// A member reading a shared office: not the owner, with the given groups.
    pub fn member(user_id: impl Into<String>, groups: Vec<String>) -> Self {
        WikiPrincipal {
            user_id: user_id.into(),
            is_owner: false,
            groups,
        }
    }

    /// The office owner — reads pass by ownership alone, no grant needed.
    pub fn owner(user_id: impl Into<String>) -> Self {
        WikiPrincipal {
            user_id: user_id.into(),
            is_owner: true,
            groups: Vec::new(),
        }
    }
}

/// A wiki read that was refused or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WikiAccessError {
    /// The caller holds no `Read` grant on this office's wiki. Distinct
    /// from "not found" so a denial is never silently indistinguishable from an
    /// empty result.
    Denied { office_id: String },
    /// The underlying read surface failed.
    Store(String),
}

impl std::fmt::Display for WikiAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WikiAccessError::Denied { office_id } => {
                write!(f, "wiki read denied for office {office_id}")
            }
            WikiAccessError::Store(m) => write!(f, "wiki read failed: {m}"),
        }
    }
}

impl std::error::Error for WikiAccessError {}

/// An access-gated view over a [`WikiReadSurface`]. Every method runs
/// the `Read` gate for the requested office before delegating; a denied read
/// never reaches the store. Bound by borrow to a read surface, a grant store,
/// and one principal — construct it per request/session.
pub struct GatedWiki<'a> {
    inner: &'a dyn WikiReadSurface,
    grants: &'a GrantStore,
    principal: WikiPrincipal,
}

impl<'a> GatedWiki<'a> {
    pub fn new(
        inner: &'a dyn WikiReadSurface,
        grants: &'a GrantStore,
        principal: WikiPrincipal,
    ) -> Self {
        GatedWiki {
            inner,
            grants,
            principal,
        }
    }

    /// The gate: `has_access(Wiki, office_id, Read)`. Owner-wins is folded
    /// in by `has_access` via `is_owner`.
    fn authorize(&self, office_id: &str) -> Result<(), WikiAccessError> {
        let allowed = self.grants.has_access(
            &self.principal.user_id,
            self.principal.is_owner,
            ResourceKind::Wiki,
            office_id,
            Permission::Read,
            &self.principal.groups,
        );
        if allowed {
            Ok(())
        } else {
            Err(WikiAccessError::Denied {
                office_id: office_id.to_string(),
            })
        }
    }

    /// One page by id within `office_id`. A page that belongs to a different
    /// office is reported as absent, so a reader authorized for one office can
    /// never read another office's page through a guessed id.
    pub fn page(&self, office_id: &str, id: &str) -> Result<Option<WikiPage>, WikiAccessError> {
        self.authorize(office_id)?;
        let page = self.inner.page(id).map_err(WikiAccessError::Store)?;
        Ok(page.filter(|p| p.office_id == office_id))
    }

    /// The direct children of `parent_id` (or the roots when `None`), for the
    /// navigation tree — gated on `office_id`.
    pub fn children(
        &self,
        office_id: &str,
        parent_id: Option<&str>,
    ) -> Result<Vec<WikiPage>, WikiAccessError> {
        self.authorize(office_id)?;
        self.inner
            .children(office_id, parent_id)
            .map_err(WikiAccessError::Store)
    }

    /// Full-text search within the office, gated on `office_id`.
    pub fn search(
        &self,
        office_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WikiPage>, WikiAccessError> {
        self.authorize(office_id)?;
        self.inner
            .search(office_id, query, limit)
            .map_err(WikiAccessError::Store)
    }

    /// Change history newest-first, gated on `office_id`.
    pub fn changelog(
        &self,
        office_id: &str,
        limit: usize,
    ) -> Result<Vec<WikiChangelogEntry>, WikiAccessError> {
        self.authorize(office_id)?;
        self.inner
            .changelog(office_id, limit)
            .map_err(WikiAccessError::Store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_sharing::{AccessGrant, PrincipalKind};
    use cronus_contract::WikiPageKind;

    /// A trivial in-memory read surface (domain cannot depend on the SQLite
    /// store); enough to prove the gate lets authorized reads through and
    /// blocks the rest.
    struct FakeReader {
        pages: Vec<WikiPage>,
    }

    impl WikiReadSurface for FakeReader {
        fn page(&self, id: &str) -> Result<Option<WikiPage>, String> {
            Ok(self.pages.iter().find(|p| p.id == id).cloned())
        }
        fn children(
            &self,
            office_id: &str,
            parent_id: Option<&str>,
        ) -> Result<Vec<WikiPage>, String> {
            Ok(self
                .pages
                .iter()
                .filter(|p| p.office_id == office_id && p.parent_id.as_deref() == parent_id)
                .cloned()
                .collect())
        }
        fn search(
            &self,
            office_id: &str,
            query: &str,
            _limit: usize,
        ) -> Result<Vec<WikiPage>, String> {
            Ok(self
                .pages
                .iter()
                .filter(|p| p.office_id == office_id && p.body.contains(query))
                .cloned()
                .collect())
        }
        fn changelog(
            &self,
            _office_id: &str,
            _limit: usize,
        ) -> Result<Vec<WikiChangelogEntry>, String> {
            Ok(vec![])
        }
    }

    fn reader() -> FakeReader {
        let overview = WikiPage::new(
            "office-1:overview",
            "office-1",
            WikiPageKind::Overview,
            "Overview",
            "the project ships value",
        );
        let other = WikiPage::new(
            "office-2:overview",
            "office-2",
            WikiPageKind::Overview,
            "Other",
            "another office",
        );
        FakeReader {
            pages: vec![overview, other],
        }
    }

    fn read_grant(user: &str) -> AccessGrant {
        AccessGrant {
            resource_type: ResourceKind::Wiki,
            resource_id: "office-1".into(),
            principal_type: PrincipalKind::User,
            principal_id: user.into(),
            permission: Permission::Read,
        }
    }

    #[test]
    fn a_read_without_the_read_grant_is_denied() {
        // A non-owner with no grant is refused on every read method.
        let reader = reader();
        let grants = GrantStore::new();
        let gate = GatedWiki::new(&reader, &grants, WikiPrincipal::member("stranger", vec![]));

        assert_eq!(
            gate.page("office-1", "office-1:overview"),
            Err(WikiAccessError::Denied {
                office_id: "office-1".into()
            })
        );
        assert!(matches!(
            gate.children("office-1", None),
            Err(WikiAccessError::Denied { .. })
        ));
        assert!(matches!(
            gate.search("office-1", "value", 10),
            Err(WikiAccessError::Denied { .. })
        ));
        assert!(matches!(
            gate.changelog("office-1", 10),
            Err(WikiAccessError::Denied { .. })
        ));
    }

    #[test]
    fn a_direct_read_grant_opens_the_office() {
        let reader = reader();
        let mut grants = GrantStore::new();
        grants.add(read_grant("alice"));
        let gate = GatedWiki::new(&reader, &grants, WikiPrincipal::member("alice", vec![]));

        let page = gate
            .page("office-1", "office-1:overview")
            .expect("granted read");
        assert_eq!(page.unwrap().title, "Overview");
        assert_eq!(gate.children("office-1", None).expect("roots").len(), 1);
        assert_eq!(
            gate.search("office-1", "value", 10).expect("search").len(),
            1
        );
    }

    #[test]
    fn the_owner_reads_without_an_explicit_grant() {
        // Ownership alone authorizes, no grant row needed.
        let reader = reader();
        let grants = GrantStore::new();
        let gate = GatedWiki::new(&reader, &grants, WikiPrincipal::owner("alice"));
        assert!(
            gate.page("office-1", "office-1:overview")
                .expect("owner read")
                .is_some()
        );
    }

    #[test]
    fn a_grant_on_one_office_does_not_leak_to_another() {
        // The grant is for office-1; reading office-2 is still denied even
        // though the same principal holds a grant elsewhere.
        let reader = reader();
        let mut grants = GrantStore::new();
        grants.add(read_grant("alice"));
        let gate = GatedWiki::new(&reader, &grants, WikiPrincipal::member("alice", vec![]));

        assert!(matches!(
            gate.page("office-2", "office-2:overview"),
            Err(WikiAccessError::Denied { .. })
        ));
    }

    #[test]
    fn a_cross_office_page_id_is_reported_absent_not_leaked() {
        // Authorized for office-1, asking for office-1 but naming office-2's
        // page id → absent, never the other office's row.
        let reader = reader();
        let mut grants = GrantStore::new();
        grants.add(read_grant("alice"));
        let gate = GatedWiki::new(&reader, &grants, WikiPrincipal::member("alice", vec![]));

        assert_eq!(
            gate.page("office-1", "office-2:overview").expect("read"),
            None,
            "a page from another office is not visible through this office's gate"
        );
    }
}
