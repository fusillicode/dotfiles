//! Overqualified fn-call rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use super::common::CallDetails;
use super::common::FnCallFinding;
use super::common::FnCallKind;
use super::common::find_fn_calls;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

pub struct OverqualifiedCallRule;

impl TypedRule for OverqualifiedCallRule {
    type Violation = OverqualifiedCallViolation;

    fn code() -> &'static str {
        "oc"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        find_fn_calls(ctx.file, FnCallKind::Overqualified)
            .into_iter()
            .map(|finding| OverqualifiedCallViolation::new(ctx.path, finding))
            .collect()
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) struct OverqualifiedCallViolation {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) details: CallDetails,
}

impl OverqualifiedCallViolation {
    fn new(path: &Path, finding: FnCallFinding) -> Self {
        let location = finding.span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            details: CallDetails {
                actual_path: finding.actual_path,
                replacement_path: finding.suggestion.expected_path,
                add_import: finding.suggestion.required_import,
            },
        }
    }
}

impl TypedRuleViolation for OverqualifiedCallViolation {
    type Rule = OverqualifiedCallRule;
}

impl Display for OverqualifiedCallViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        let details = format!(
            "replace `{}` with `{}`{}",
            self.details.actual_path,
            self.details.replacement_path,
            self.details
                .add_import
                .as_ref()
                .map_or_else(String::new, |import| format!("; add `{import}`")),
        );
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            OverqualifiedCallRule::code(),
            &details,
        ))
    }
}
