//! A limit on how much work one answer may do.
//!
//! A session can hold gigabytes of OS trace, and an agent's context window is small. Scans take a
//! [`Budget`] and stop when it runs out; a command then either returns what it has marked
//! `truncated` with a cursor, or fails with the "over budget" exit code if a partial answer would
//! mislead.
//!
//! A [`Budget`] has three limits: a deadline, a number of bytes read, and a flag that something else
//! (another thread, a signal handler) can set to cancel the work. A scan calls
//! [`Budget::charge`] with the size of each record it reads, or [`Budget::check`] where it reads
//! none, and the first [`Exceeded`] it gets is its cue to stop. A budget is a value, not a thread:
//! nothing interrupts a scan that does not ask, and the limits are enforced at the granularity the
//! scan checks.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

/// Which limit of a [`Budget`] was hit.
///
/// Its [`Display`](fmt::Display) completes a sentence about the work: `the search ran out of time`.
/// A command turns it into an "over budget" failure (the `Budget` exit kind of the `envelope`
/// module) with that sentence and a hint on how to narrow the request.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::budget::Exceeded;
///
/// assert_eq!(Exceeded::Time.to_string(), "ran out of time");
/// assert_eq!(Exceeded::Bytes.to_string(), "read more data than the budget allows");
/// assert_eq!(Exceeded::Cancelled.to_string(), "was cancelled");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exceeded {
    /// The deadline passed.
    Time,
    /// More bytes were charged than the budget reads at most.
    Bytes,
    /// The budget's cancel flag was set.
    Cancelled,
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Time => "ran out of time",
            Self::Bytes => "read more data than the budget allows",
            Self::Cancelled => "was cancelled",
        })
    }
}

impl std::error::Error for Exceeded {}

/// How much time and data a scan may use, and a way to cancel it.
///
/// The default is [`Budget::unlimited`]. Limits are added with [`Budget::within`] and
/// [`Budget::reading_at_most`]. Each returns the budget, so they chain.
///
/// A clone is another handle on the same budget as far as bytes and cancellation go: the count of
/// bytes charged and the cancel flag are shared between a budget and every clone of it, so a limit
/// applies across the threads a scan is split over. The deadline and the byte limit are copied.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::budget::{Budget, Exceeded};
///
/// // a scan may read 100 bytes; 60 and then 60 more is too much
/// let budget = Budget::unlimited().reading_at_most(100);
/// assert_eq!(budget.charge(60), Ok(()));
/// assert_eq!(budget.charge(60), Err(Exceeded::Bytes));
/// assert_eq!(budget.spent_bytes(), 120);
///
/// // clones share the count and the cancel flag
/// let budget = Budget::unlimited();
/// let worker = budget.clone();
/// budget
///     .cancel_handle()
///     .store(true, std::sync::atomic::Ordering::Relaxed);
/// assert_eq!(worker.check(), Err(Exceeded::Cancelled));
/// ```
#[derive(Debug, Clone)]
pub struct Budget {
    deadline: Option<Instant>,
    max_bytes: Option<u64>,
    spent_bytes: Arc<AtomicU64>,
    cancelled: Arc<AtomicBool>,
}

impl Default for Budget {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl Budget {
    /// Returns a budget with no deadline and no byte limit. Only [`Budget::cancel_handle`] can
    /// make it run out.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            deadline: None,
            max_bytes: None,
            spent_bytes: Arc::new(AtomicU64::new(0)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns the budget with a deadline `time` from now.
    ///
    /// The clock starts when this is called, not when the scan starts, so build the budget just
    /// before the scan. A second call replaces the deadline.
    #[must_use]
    pub fn within(mut self, time: Duration) -> Self {
        self.deadline = Some(Instant::now() + time);
        self
    }

    /// Returns the budget with a limit of `bytes` that may be charged in total, across every clone
    /// of it. A second call replaces the limit.
    ///
    /// The limit is exceeded when the total is more than `bytes`; charging exactly `bytes` is
    /// within the budget.
    #[must_use]
    pub fn reading_at_most(mut self, bytes: u64) -> Self {
        self.max_bytes = Some(bytes);
        self
    }

    /// Returns a handle that cancels this budget (and every clone of it) when set.
    ///
    /// Store `true` in it, from any thread, and the next [`Budget::check`] or [`Budget::charge`] on
    /// the budget or a clone fails with [`Exceeded::Cancelled`].
    #[must_use]
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }

    /// Returns an error if the work was cancelled or the deadline passed, and `Ok` if the work may
    /// go on. Cheap enough to call per record.
    ///
    /// # Errors
    ///
    /// - [`Exceeded::Cancelled`] if the cancel flag was set. This is checked first.
    /// - [`Exceeded::Time`] if the deadline has passed. An unlimited budget has none.
    ///
    /// The byte limit is not looked at; [`Budget::charge`] does that.
    pub fn check(&self) -> Result<(), Exceeded> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(Exceeded::Cancelled);
        }

        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(Exceeded::Time);
        }

        Ok(())
    }

    /// Accounts for `bytes` read, then does what [`Budget::check`] does.
    ///
    /// The bytes are counted even when the answer is an error, so [`Budget::spent_bytes`] includes
    /// the charge that went over the limit.
    ///
    /// # Errors
    ///
    /// - [`Exceeded::Bytes`] if the total charged, with these `bytes`, is more than the limit of
    ///   [`Budget::reading_at_most`]. This is checked before the cancel flag and the deadline,
    ///   so a budget that is over its limit and also cancelled says `Bytes`.
    /// - [`Exceeded::Cancelled`] or [`Exceeded::Time`], as for [`Budget::check`].
    pub fn charge(&self, bytes: u64) -> Result<(), Exceeded> {
        let spent = self.spent_bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
        if self.max_bytes.is_some_and(|max| spent > max) {
            return Err(Exceeded::Bytes);
        }

        self.check()
    }

    /// Returns the total number of bytes charged so far, across this budget and every clone of it.
    #[must_use]
    pub fn spent_bytes(&self) -> u64 {
        self.spent_bytes.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unlimited_budget_never_runs_out() {
        let budget = Budget::unlimited();
        for _ in 0..1000 {
            assert_eq!(budget.charge(1 << 30), Ok(()));
        }
    }

    #[test]
    fn bytes_are_counted_across_clones() {
        let budget = Budget::unlimited().reading_at_most(100);
        let clone = budget.clone();
        assert_eq!(budget.charge(60), Ok(()));
        assert_eq!(clone.charge(60), Err(Exceeded::Bytes));
        assert_eq!(budget.spent_bytes(), 120);
    }

    #[test]
    fn time_runs_out() {
        let budget = Budget::unlimited().within(Duration::ZERO);
        assert_eq!(budget.check(), Err(Exceeded::Time));
    }

    #[test]
    fn cancelling_a_handle_stops_every_clone() {
        let budget = Budget::unlimited();
        let clone = budget.clone();
        budget.cancel_handle().store(true, Ordering::Relaxed);
        assert_eq!(clone.check(), Err(Exceeded::Cancelled));
    }
}
