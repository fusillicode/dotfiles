//! Render structured findings against the exact source snapshot inspected by the report pass.

use std::path::Path;
use std::path::PathBuf;

use super::analysis::Finding;
use super::analysis::FixStatus;

#[derive(Default)]
pub struct RslOutput {
    pub(super) reports: Vec<FileReport>,
}

impl RslOutput {
    pub fn render(&self) -> String {
        self.reports
            .iter()
            .flat_map(|report| report.findings.iter().map(|finding| finding.render(&report.path)))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn is_empty(&self) -> bool {
        self.reports.iter().all(|report| report.findings.is_empty())
    }
}

pub(super) struct FileReport {
    pub(super) path: PathBuf,
    pub(super) findings: Vec<RuleFinding>,
}

pub(super) struct RuleFinding {
    pub(super) rule_id: &'static str,
    pub(super) finding: Finding,
}

impl RuleFinding {
    fn render(&self, path: &Path) -> String {
        let location = format!(
            "{}:{}:{}",
            path.display(),
            self.finding.position.line,
            self.finding.position.column,
        );

        match &self.finding.status {
            FixStatus::Fixable => format!(
                "{location}: available [{}]: {}\n  Help: Run with --mode write to apply this change.",
                self.rule_id, self.finding.description,
            ),
            FixStatus::Blocked(reason) => format!(
                "{location}: skipped [{}/{}]: {}\n  Reason: {}\n  Help: {}",
                self.rule_id, reason.code, self.finding.description, reason.message, reason.help,
            ),
        }
    }
}
