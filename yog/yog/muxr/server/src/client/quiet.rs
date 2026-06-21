// Quiet that becomes ready on a loop boundary is deferred long enough to drain queued PTY output before clearing Busy.
// Requests and output arm the same step because either can be selected just as the quiet sleep becomes ready.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum QuietTurn {
    #[default]
    Idle,
    DrainBeforeClear,
}

impl QuietTurn {
    pub(super) const fn after_request(&mut self, quiet_elapsed: bool) {
        if quiet_elapsed {
            *self = Self::DrainBeforeClear;
        }
    }

    pub(super) const fn after_output(&mut self, quiet_elapsed: bool) {
        if quiet_elapsed {
            *self = Self::DrainBeforeClear;
        }
    }

    pub(super) fn take_ready(&mut self, quiet_elapsed: bool) -> Self {
        let ready = std::mem::take(self);
        if quiet_elapsed { ready } else { Self::Idle }
    }

    pub(super) const fn drains_before_clear(self) -> bool {
        matches!(self, Self::DrainBeforeClear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quiet_turn_after_request_drains_before_clear() {
        let mut quiet_turn = QuietTurn::default();

        quiet_turn.after_request(true);

        pretty_assertions::assert_eq!(quiet_turn.take_ready(true), QuietTurn::DrainBeforeClear);
        pretty_assertions::assert_eq!(quiet_turn.take_ready(true), QuietTurn::Idle);
    }

    #[test]
    fn test_quiet_turn_after_output_drains_before_clear() {
        let mut quiet_turn = QuietTurn::default();

        quiet_turn.after_output(true);

        pretty_assertions::assert_eq!(quiet_turn.take_ready(true), QuietTurn::DrainBeforeClear);
        pretty_assertions::assert_eq!(quiet_turn.take_ready(true), QuietTurn::Idle);
    }

    #[test]
    fn test_quiet_turn_when_not_elapsed_does_not_run() {
        let mut quiet_turn = QuietTurn::default();

        quiet_turn.after_request(true);

        pretty_assertions::assert_eq!(quiet_turn.take_ready(false), QuietTurn::Idle);
        pretty_assertions::assert_eq!(quiet_turn.take_ready(true), QuietTurn::Idle);
    }

    #[test]
    fn test_quiet_turn_when_elapsed_is_false_does_not_arm() {
        let mut quiet_turn = QuietTurn::default();

        quiet_turn.after_request(false);
        quiet_turn.after_output(false);

        pretty_assertions::assert_eq!(quiet_turn.take_ready(true), QuietTurn::Idle);
    }
}
