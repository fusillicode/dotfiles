//! Output rendering for the `frs rsl` command.

pub(super) use self::format::FormattedRuleViolation;
use super::rules::RuleViolation;

mod format;

#[derive(Clone, Copy)]
pub(super) enum ViolationOutputFormat {
    Compact,
    Debug,
}

pub struct RslOutput {
    violations: Vec<Box<dyn RuleViolation>>,
    format: ViolationOutputFormat,
}

impl RslOutput {
    pub(super) const fn new(violations: Vec<Box<dyn RuleViolation>>, format: ViolationOutputFormat) -> Self {
        Self { violations, format }
    }

    pub const fn is_empty(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn render(&self) -> String {
        self.violations
            .iter()
            .map(|violation| violation.render(self.format))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
