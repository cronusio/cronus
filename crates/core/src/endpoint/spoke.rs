//! Making a composition a spoke: the same catalog, with the handlers of the
//! loci that run in the engine re-pointed at it.
//!
//! A spoke composes exactly as an engine does, so its descriptors, binders and
//! help are the engine's (the exact-version rule is what makes that safe).
//! Only what *runs* changes. `ClientLocal` and `HostOnly` actions stay where
//! they are: they change what this client shows and touch nothing the engine
//! owns.

use std::sync::Arc;

use cronus_contract::{Binder, InvocableId, Invocation, LocusKind, Outcome, Surface};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, StreamHub};

use super::client::{Answer, Link, Piece, Streaming};

/// The loci that run in the engine.
const FORWARDED: [LocusKind; 2] = [LocusKind::Semantic, LocusKind::Installation];

/// Re-point every forwarded handler of `dispatcher` at the engine `link`
/// reaches. `caller` is the surface this spoke is, named to the engine so its
/// journal says who asked.
pub fn point_at_engine(
    registry: &InvocableRegistry,
    dispatcher: &mut Dispatcher,
    link: &Arc<Link>,
    caller: Surface,
) {
    let hub = dispatcher.streams();
    let targets: Vec<(InvocableId, Vec<Binder>)> = registry
        .all()
        .filter(|invocable| FORWARDED.contains(&invocable.locus.kind()))
        .map(|invocable| (invocable.id.clone(), invocable.binders.clone()))
        .collect();
    for (id, binders) in targets {
        let link = Arc::clone(link);
        let hub = Arc::clone(&hub);
        let named = id.clone();
        dispatcher.attach(
            id,
            Arc::new(move |args| {
                forward(
                    &link,
                    &hub,
                    &Invocation {
                        id: named.clone(),
                        args: args.clone(),
                        caller,
                    },
                    &binders,
                )
            }),
        );
    }
}

fn engine_did_not_answer(link: &Link, why: impl std::fmt::Display) -> Outcome {
    Outcome::Unavailable {
        reason: format!("the engine at {} did not answer: {why}", link.addr()),
    }
}

/// Run one invocation at the engine and bring its answer back as an outcome.
fn forward(
    link: &Arc<Link>,
    hub: &Arc<StreamHub>,
    invocation: &Invocation,
    binders: &[Binder],
) -> Outcome {
    // The caller's directory travels with the invocation: a relative path in it
    // means this process's, not the engine's.
    match link.invoke(invocation, binders, &cronus_domain::paths::caller_dir()) {
        Ok(Answer::Done(outcome)) => outcome,
        // The local catalog resolved this identity, so an engine that does not
        // know it is a different build from this one: said, not folded into an
        // empty result and not quietly run here.
        Ok(Answer::Unknown) => Outcome::Unavailable {
            reason: format!(
                "the engine does not know {}; it is not the same release as this one",
                invocation.id
            ),
        },
        Ok(Answer::Streaming(streaming)) => {
            let (handle, sink) = hub.open();
            let pumping = Arc::clone(link);
            let spawned = std::thread::Builder::new()
                .name("engine-stream".to_string())
                .spawn(move || pump(&pumping, streaming, sink));
            match spawned {
                Ok(_) => Outcome::Stream(handle),
                Err(err) => engine_did_not_answer(link, err),
            }
        }
        Err(err) => engine_did_not_answer(link, err),
    }
}

/// Carry a streamed answer from the connection into a local stream. When the
/// connection fails the local stream ends saying so, whatever the engine goes
/// on to do: the work behind it runs to its end and is recorded there.
fn pump(link: &Link, mut streaming: Streaming, sink: cronus_domain::invocable::StreamSink) {
    loop {
        match streaming.next_piece() {
            Ok(Piece::Chunk(value)) => {
                sink.chunk(value);
            }
            Ok(Piece::End(outcome)) => {
                sink.end(outcome);
                return;
            }
            Err(err) => {
                sink.end(engine_did_not_answer(link, err));
                return;
            }
        }
    }
}
