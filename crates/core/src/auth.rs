//! Multi-user authentication — moved wholesale to `cronus-auth-local`:
//! every operation touches `bcrypt`/`hmac`/`sha1`/`getrandom`, and
//! nothing in the domain tier references `PrivilegeMap`/`RESERVED_USERNAMES`
//! directly (only frontends, through this facade re-export).

pub use cronus_auth_local::{
    AuthError, AuthStore, DeveloperAdmissionStore, HumanPrincipal, PrivilegeMap,
    RESERVED_USERNAMES, SessionStore, SingleUserIdentity, login,
};
