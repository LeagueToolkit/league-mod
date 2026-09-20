//! [`Cancellation`]: whether the caller has asked an operation to stop.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the caller has asked a long operation to stop.
///
/// An operation taking one of these checks it between its steps, so a
/// cancellation lands between files rather than part-way through one, and fails
/// with its own cancelled error.
///
/// The usual source is an `AtomicBool` shared with whatever drives the UI, so
/// `From<&AtomicBool>` is the conversion a caller normally needs and a builder
/// taking `impl Into<Cancellation>` takes an `&Arc<AtomicBool>` deref directly.
/// [`predicate`](Self::predicate) covers a caller whose answer comes from
/// somewhere else.
///
/// # Example
///
/// ```
/// use std::sync::atomic::{AtomicBool, Ordering};
/// use ltk_mod_project::Cancellation;
///
/// let flag = AtomicBool::new(false);
/// let cancellation = Cancellation::from(&flag);
/// assert!(!cancellation.is_cancelled());
///
/// flag.store(true, Ordering::Relaxed);
/// assert!(cancellation.is_cancelled());
/// ```
#[derive(Clone, Copy, Default)]
pub struct Cancellation<'a>(Source<'a>);

#[derive(Clone, Copy, Default)]
enum Source<'a> {
    #[default]
    Never,
    Flag(&'a AtomicBool),
    Predicate(&'a (dyn Fn() -> bool + Sync)),
}

impl<'a> Cancellation<'a> {
    /// A cancellation nothing ever sets, for an operation the caller cannot
    /// stop. This is the default.
    pub const NEVER: Self = Self(Source::Never);

    /// Cancelled once `flag` is set.
    ///
    /// The flag is read with [`Ordering::Relaxed`]: a cancellation only has to
    /// be noticed eventually, and nothing is published alongside it.
    pub const fn flag(flag: &'a AtomicBool) -> Self {
        Self(Source::Flag(flag))
    }

    /// Cancelled once `cancelled` answers `true`.
    ///
    /// It is called once per step of whatever it was given to, so it should be
    /// cheap and must not block.
    pub const fn predicate(cancelled: &'a (dyn Fn() -> bool + Sync)) -> Self {
        Self(Source::Predicate(cancelled))
    }

    /// Whether the operation has been asked to stop.
    pub fn is_cancelled(&self) -> bool {
        match self.0 {
            Source::Never => false,
            Source::Flag(flag) => flag.load(Ordering::Relaxed),
            Source::Predicate(cancelled) => cancelled(),
        }
    }
}

impl<'a> From<&'a AtomicBool> for Cancellation<'a> {
    fn from(flag: &'a AtomicBool) -> Self {
        Self::flag(flag)
    }
}

impl fmt::Debug for Cancellation<'_> {
    /// Prints the answer rather than the source, since a predicate has nothing
    /// printable and the answer is what a reader of a log wants.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Cancellation")
            .field(&self.is_cancelled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_never_cancelled() {
        assert!(!Cancellation::default().is_cancelled());
        assert!(!Cancellation::NEVER.is_cancelled());
    }

    #[test]
    fn a_flag_is_read_every_time_it_is_asked() {
        let flag = AtomicBool::new(false);
        let cancellation = Cancellation::flag(&flag);

        assert!(!cancellation.is_cancelled());
        flag.store(true, Ordering::Relaxed);
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn a_predicate_answers_for_itself() {
        let cancelled = || true;
        assert!(Cancellation::predicate(&cancelled).is_cancelled());
    }
}
