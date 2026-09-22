//! The copies of the picture being prepared on threads of their own: a
//! copy of the picture has to walk every pixel before it can say whether
//! it worked, and the thread doing that has no business touching the
//! interface. So each is given a ticket, reports through it, and the loop
//! picks the reports up on the same cadence it looks at the file, the
//! palette and the clipboard on.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

/// What a copy prepared on a thread of its own did: took the selection, and
/// this is what to say about it, or failed with this much to say about it.
/// `Err` carries the one line the window shows; the whole chain has already
/// gone to the terminal.
pub(super) type Outcome = Result<&'static str, String>;

/// What a thread preparing a copy holds: which copy it is, the count to
/// tell whether it has been superseded, and the way home for its report.
pub(super) struct Ticket {
    asked: u64,
    count: Arc<AtomicU64>,
    outcome: Sender<Outcome>,
}

impl Ticket {
    /// Whether something has been copied since this was asked for. Taking
    /// the selection then would put back a picture the user has already
    /// moved on from: copying a large image takes long enough for a second
    /// press to arrive while the first is still working, and the clipboard
    /// should end up holding the one asked for last rather than whichever
    /// finished last.
    pub fn superseded(&self) -> bool {
        self.count.load(Ordering::Relaxed) != self.asked
    }

    /// Says how the copy went. Nobody listening — the window gone — is
    /// nothing to do anything about.
    pub fn report(&self, outcome: Outcome) {
        let _ = self.outcome.send(outcome);
    }
}

pub(super) struct Copying {
    outcome: (Sender<Outcome>, Receiver<Outcome>),
    /// The copies still being prepared, joined before the loop leaves. A
    /// copy is often followed straight away by `q`, and a thread that has
    /// not yet handed its bytes over dies with the process — the copy would
    /// go missing for no reason the user could see.
    threads: Vec<JoinHandle<()>>,
    /// Counts copies asked for, shared with the threads doing the work.
    count: Arc<AtomicU64>,
}

impl Default for Copying {
    fn default() -> Self {
        Self {
            outcome: mpsc::channel(),
            threads: Vec::new(),
            count: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Copying {
    /// Marks a copy as the one most recently asked for — one done in line,
    /// with nothing to prepare — and says which number it is.
    pub fn claim(&self) -> u64 {
        self.count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Starts `work` on a thread of its own, with the ticket it reports
    /// through. Threads that have already handed their bytes over are
    /// dropped as each new copy is asked for, so the list is what is still
    /// in flight rather than every copy the session has ever made.
    pub fn spawn(&mut self, work: impl FnOnce(Ticket) + Send + 'static) {
        let ticket = Ticket {
            asked: self.claim(),
            count: Arc::clone(&self.count),
            outcome: self.outcome.0.clone(),
        };
        self.threads.retain(|thread| !thread.is_finished());
        self.threads.push(std::thread::spawn(move || work(ticket)));
    }

    /// The reports that have come in since the last look.
    pub fn poll(&mut self) -> Vec<Outcome> {
        self.outcome.1.try_iter().collect()
    }

    /// Waits for every copy still being prepared.
    pub fn join_all(&mut self) {
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy reports through its ticket, and the report is there at the
    /// next look; a copy asked for after it supersedes it.
    #[test]
    fn a_copy_reports_and_a_later_one_supersedes_it() {
        let mut copying = Copying::default();
        let (started, wait) = mpsc::channel::<()>();
        copying.spawn(move |ticket| {
            wait.recv().expect("told to go on");
            if ticket.superseded() {
                ticket.report(Err("superseded".to_string()));
            } else {
                ticket.report(Ok("Copied."));
            }
        });
        assert!(copying.poll().is_empty(), "nothing reported yet");
        copying.claim();
        started.send(()).expect("the thread is waiting");
        copying.join_all();
        assert_eq!(copying.poll(), [Err("superseded".to_string())]);

        copying.spawn(|ticket| ticket.report(Ok("Copied.")));
        copying.join_all();
        assert_eq!(copying.poll(), [Ok("Copied.")]);
        assert!(copying.poll().is_empty(), "each report is taken once");
    }
}
