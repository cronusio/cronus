//! The invocable registry and its dispatch: one registration door for every
//! action the core and its extensions expose, and the bind-before-invoke
//! rule that runs them.
//!
//! Pure and I/O-free — registration, binding, and lookup are in-memory
//! bookkeeping with no adapter behind them. The facade calls the same
//! [`InvocableRegistry::register`] for core invocables that an extension
//! loader calls for contributed ones; there is no second, privileged path.
//!
//! Two concerns, two files: the catalog ([`registry`]) and its identity
//! rules, and execution ([`dispatch`]) and its binding rule.

mod dispatch;
mod registry;
mod stream;

pub use dispatch::{
    CONTRIBUTION_TIME_BOUND, DispatchHandle, DispatchJournal, Dispatcher, Handler, JournalRecord,
    JournalSink, bind,
};
pub use registry::{
    CONTRIBUTE_GRANT, CORE_IDENTITY, DescriptorFieldError, InvocableRegistry, Registrant,
    Registration, RegistrationError, RegistrationHandle, RegistryObserver, Shadow, attribution,
};
pub use stream::{Polled, StreamHub, StreamItem, StreamSink, Subscription};
