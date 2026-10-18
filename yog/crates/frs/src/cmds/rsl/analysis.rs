//! A rule analyzes one immutable source snapshot and returns planned edits and findings.
//!
//! Rules do not know whether the caller will apply their edits or only report their findings.

use proc_macro2::Span;

use super::source::SourceEditor;
use super::source::SourcePosition;

/// The rule owns the data and description of its proposed change.
pub(super) trait RuleChange {
    fn span(&self) -> Span;

    fn description(&self) -> String;
}

/// A rule-owned explanation, converted to display data at the reporting boundary.
pub(super) struct FixBlockReason {
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) help: &'static str,
}

pub(super) struct RulePlan<'source> {
    pub(super) edits: SourceEditor<'source>,
    pub(super) findings: Vec<Finding>,
}

impl RulePlan<'_> {
    pub(super) fn record_fixable(&mut self, change: &impl RuleChange) {
        self.record(change, FixStatus::Fixable);
    }

    pub(super) fn record_blocked(&mut self, change: &impl RuleChange, reason: impl Into<FixBlockReason>) {
        self.record(change, FixStatus::Blocked(reason.into()));
    }

    pub(super) fn apply_edits(self) -> rootcause::Result<String> {
        self.edits.finish()
    }

    pub(super) fn into_findings(self) -> Vec<Finding> {
        self.findings
    }

    fn record(&mut self, change: &impl RuleChange, status: FixStatus) {
        let start = change.span().start();
        self.findings.push(Finding {
            position: SourcePosition {
                line: start.line,
                column: start.column.saturating_add(1),
            },
            description: change.description(),
            status,
        });
    }
}

pub(super) struct Finding {
    pub(super) position: SourcePosition,
    pub(super) description: String,
    pub(super) status: FixStatus,
}

pub(super) enum FixStatus {
    Fixable,
    Blocked(FixBlockReason),
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    use super::*;
    use crate::cmds::rsl::output::FileReport;
    use crate::cmds::rsl::output::RslOutput;
    use crate::cmds::rsl::output::RuleFinding;

    struct CustomChange {
        span: Span,
        original: &'static str,
        replacement: &'static str,
    }

    impl RuleChange for CustomChange {
        fn span(&self) -> Span {
            self.span
        }

        fn description(&self) -> String {
            format!("replace `{}` with `{}`", self.original, self.replacement)
        }
    }

    struct CustomReason;

    impl From<CustomReason> for FixBlockReason {
        fn from(_: CustomReason) -> Self {
            Self {
                code: "custom-context",
                message: "This rule needs its own context.".to_owned(),
                help: "Supply that context.",
            }
        }
    }

    #[test]
    fn test_rule_plan_when_a_rule_defines_its_own_types_reports_without_shared_variants() {
        let change = CustomChange {
            span: Span::call_site(),
            original: "old",
            replacement: "new",
        };
        let mut plan = RulePlan {
            edits: SourceEditor::new("old"),
            findings: Vec::new(),
        };
        plan.record_blocked(&change, CustomReason);
        let output = RslOutput {
            reports: vec![FileReport {
                path: PathBuf::from("input.rs"),
                findings: plan
                    .into_findings()
                    .into_iter()
                    .map(|finding| RuleFinding {
                        rule_id: "custom",
                        finding,
                    })
                    .collect(),
            }],
        };

        assert_eq!(
            output.render(),
            "input.rs:1:1: skipped [custom/custom-context]: replace `old` with `new`\n  \
         Reason: This rule needs its own context.\n  Help: Supply that context.",
        );
    }
}
