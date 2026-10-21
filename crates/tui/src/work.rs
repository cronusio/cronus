//! Work done off the render thread.
//!
//! A core call can take as long as the disk and the model make it, and the
//! screen must keep answering keys meanwhile. Everything the loop needs from
//! the core therefore runs on a thread of its own and reports back through one
//! channel; the loop drains the channel at the start of each tick and applies
//! what finished to the view-model. Each job names its kind so that a job that
//! dies without answering still clears the "in flight" mark it set — a command
//! that panicked must not leave the command bar refusing every later line.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, mpsc};

use cronus_core::invocable::{Dispatcher, InvocableRegistry};

use crate::installation::Question;
use crate::view::{Projection, ResultBlock, TranscriptView};

/// What a command left to show.
#[derive(Debug)]
pub(crate) enum CommandResult {
    /// More than one line: an installation verb's answer.
    Block(ResultBlock),
    /// The answer to a change that was agreed to and carried out. `relaunch`
    /// asks for this frontend to be rebuilt onto the same view.
    Applied { block: ResultBlock, relaunch: bool },
    /// One line for the ticker, or nothing when the line was ordinary input.
    Feedback(Option<String>),
    /// A change this surface carries out, waiting for the person to say yes.
    Ask(Question),
}

/// The kinds of job, so a failed job can be told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Command,
    Transcript,
    ChatTranscript,
    Stop,
}

/// A job's answer.
#[derive(Debug)]
pub(crate) enum Finished {
    /// A slash command ran.
    Command(CommandResult),
    /// A session's conversation was read for the Sessions screen, under the
    /// key (session and entry count) it was read for.
    Transcript {
        key: String,
        projection: Projection<TranscriptView>,
    },
    /// The Chat screen's conversation was read back from its journal.
    ChatTranscript(Projection<TranscriptView>),
    /// A stop request was answered; `Some` is a reason to tell the person.
    Stopped(Option<String>),
    /// The job ended without an answer.
    Failed(Kind),
}

/// The channel jobs report through, with the means to start one.
pub(crate) struct Work {
    sender: mpsc::Sender<Finished>,
    receiver: mpsc::Receiver<Finished>,
}

impl Work {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    /// Run `job` on a thread of its own against the shared registry and
    /// dispatcher. A job that panics reports [`Finished::Failed`] for `kind`
    /// instead of vanishing.
    pub(crate) fn spawn<F>(
        &self,
        kind: Kind,
        registry: &Arc<InvocableRegistry>,
        dispatcher: &Arc<Dispatcher>,
        job: F,
    ) where
        F: FnOnce(&InvocableRegistry, &Dispatcher) -> Finished + Send + 'static,
    {
        let (registry, dispatcher) = (Arc::clone(registry), Arc::clone(dispatcher));
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let finished = catch_unwind(AssertUnwindSafe(|| job(&registry, &dispatcher)))
                .unwrap_or(Finished::Failed(kind));
            // The loop having gone away is the only way this fails, and then
            // nobody is waiting for the answer.
            let _ = sender.send(finished);
        });
    }

    /// The next finished job, if there is one now.
    pub(crate) fn next(&self) -> Option<Finished> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(work: &Work) -> Finished {
        for _ in 0..400 {
            if let Some(finished) = work.next() {
                return finished;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the job never reported");
    }

    #[test]
    fn a_job_reports_its_answer_to_the_loop() {
        let work = Work::new();
        let (registry, dispatcher) = (
            Arc::new(InvocableRegistry::new()),
            Arc::new(Dispatcher::new()),
        );
        work.spawn(Kind::Command, &registry, &dispatcher, |_, _| {
            Finished::Command(CommandResult::Feedback(Some("done".to_string())))
        });
        match wait_for(&work) {
            Finished::Command(CommandResult::Feedback(Some(text))) => assert_eq!(text, "done"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_job_that_panics_still_reports_which_kind_failed() {
        let work = Work::new();
        let (registry, dispatcher) = (
            Arc::new(InvocableRegistry::new()),
            Arc::new(Dispatcher::new()),
        );
        work.spawn(Kind::Stop, &registry, &dispatcher, |_, _| {
            panic!("a handler blew up")
        });
        assert!(matches!(wait_for(&work), Finished::Failed(Kind::Stop)));
    }

    #[test]
    fn nothing_finished_is_nothing_to_apply() {
        assert!(Work::new().next().is_none());
    }
}
