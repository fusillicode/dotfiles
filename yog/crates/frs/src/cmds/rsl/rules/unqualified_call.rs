//! Unqualified function-call rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use serde::Serialize;

use super::common::CallDetails;
use super::common::FunctionCallFinding;
use super::common::FunctionCallKind;
use super::common::find_function_calls;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

pub struct UnqualifiedCallRule;

impl TypedRule for UnqualifiedCallRule {
    type Violation = UnqualifiedCallViolation;

    fn name() -> &'static str {
        "unqualified_call"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        find_function_calls(ctx.file, FunctionCallKind::Unqualified)
            .into_iter()
            .map(|finding| UnqualifiedCallViolation::new(ctx.path, finding))
            .collect()
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
pub(super) struct UnqualifiedCallViolation {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) message: &'static str,
    pub(super) details: CallDetails,
}

impl UnqualifiedCallViolation {
    fn new(path: &Path, finding: FunctionCallFinding) -> Self {
        let location = finding.span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "call needs qualification",
            details: CallDetails {
                actual_path: finding.actual_path,
                replacement_path: finding.suggestion.expected_path,
                add_import: finding.suggestion.required_import,
            },
        }
    }
}

impl TypedRuleViolation for UnqualifiedCallViolation {
    type Rule = UnqualifiedCallRule;
}

impl Display for UnqualifiedCallViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        let details = format!(
            "replace {} with {}{}",
            self.details.actual_path,
            self.details.replacement_path,
            self.details
                .add_import
                .as_ref()
                .map_or_else(String::new, |import| format!("; add {import}")),
        );
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            self.message,
            &details,
        ))
    }
}
