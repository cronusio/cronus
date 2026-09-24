//! Model-unforgeable per-action execution receipts: a keyed-
//! BLAKE3 MAC over a length-prefixed, injective action binding.
//!
//! Everything here is pure and I/O-free, testable against fixed key
//! vectors with no dependency on the OS clock or CSPRNG. The one
//! non-deterministic act — reading OS entropy to birth a session's
//! [`ReceiptKey`] — lives in the `crates/core` facade
//! (`receipts_bootstrap.rs`), the `*_bootstrap.rs` precedent, since
//! `getrandom` is deliberately absent from this crate's boundary-guard
//! allowlist.
//!
//! Five concerns, five files: the key ([`key`]), the canonical binding
//! ([`binding`]), the MAC ([`mac`]), the structural per-action wrapper
//! ([`receipted`]), and the per-turn ledger ([`ledger`]) plus its deferred
//! ([`deferred`]) lifecycle.

mod binding;
mod deferred;
mod key;
mod ledger;
mod mac;
mod receipted;

pub use binding::{ActionBinding, digest, encode};
pub use deferred::{defer, resolve_deferred};
pub use key::ReceiptKey;
pub use ledger::{CoverageReport, ReceiptLedger, ReceiptStatus};
pub use mac::{Receipt, mint, verify};
pub use receipted::{Receipted, mint_receipted};
