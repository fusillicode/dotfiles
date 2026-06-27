use crate::client::timers::QuietDeadline;

// Quiet that becomes ready on a loop boundary is deferred long enough to drain queued PTY output before clearing Busy.
// Requests and output arm the same step because either can be selected just as the quiet sleep becomes ready.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum QuietTurn {
    #[default]
    Idle,
    DrainBeforeClear,
}

impl QuietTurn {
    pub(super) const fn defer_if_elapsed(&mut self, quiet_deadline: QuietDeadline) {
        if matches!(quiet_deadline, QuietDeadline::Elapsed) {
            *self = Self::DrainBeforeClear;
        }
    }

    pub(super) fn take_ready(&mut self, quiet_deadline: QuietDeadline) -> Self {
        let ready = std::mem::take(self);
        if matches!(quiet_deadline, QuietDeadline::Elapsed) {
            ready
        } else {
            Self::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quiet_turn_defer_if_elapsed_drains_before_clear() {
        let mut quiet_turn = QuietTurn::default();

        quiet_turn.defer_if_elapsed(QuietDeadline::Elapsed);

        pretty_assertions::assert_eq!(
            quiet_turn.take_ready(QuietDeadline::Elapsed),
            QuietTurn::DrainBeforeClear
        );
        pretty_assertions::assert_eq!(quiet_turn.take_ready(QuietDeadline::Elapsed), QuietTurn::Idle);
    }

    #[test]
    fn test_quiet_turn_defer_if_elapsed_is_idempotent() {
        let mut quiet_turn = QuietTurn::default();

        quiet_turn.defer_if_elapsed(QuietDeadline::Elapsed);
        quiet_turn.defer_if_elapsed(QuietDeadline::Elapsed);

        pretty_assertions::assert_eq!(
            quiet_turn.take_ready(QuietDeadline::Elapsed),
            QuietTurn::DrainBeforeClear
        );
        pretty_assertions::assert_eq!(quiet_turn.take_ready(QuietDeadline::Elapsed), QuietTurn::Idle);
    }

    #[test]
    fn test_quiet_turn_when_not_elapsed_does_not_run() {
        let mut quiet_turn = QuietTurn::default();

        quiet_turn.defer_if_elapsed(QuietDeadline::Elapsed);

        pretty_assertions::assert_eq!(quiet_turn.take_ready(QuietDeadline::Pending), QuietTurn::Idle);
        pretty_assertions::assert_eq!(quiet_turn.take_ready(QuietDeadline::Elapsed), QuietTurn::Idle);
    }

    #[test]
    fn test_quiet_turn_when_elapsed_is_false_does_not_arm() {
        let mut quiet_turn = QuietTurn::default();

        quiet_turn.defer_if_elapsed(QuietDeadline::Pending);
        quiet_turn.defer_if_elapsed(QuietDeadline::Pending);

        pretty_assertions::assert_eq!(quiet_turn.take_ready(QuietDeadline::Elapsed), QuietTurn::Idle);
    }
}
