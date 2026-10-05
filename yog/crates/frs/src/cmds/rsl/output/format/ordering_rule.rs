use super::FormattedRuleViolation;
use super::ViolationOutputFormat;
use super::format_violation;
use crate::cmds::rsl::rules::ordering_rule::OrderingOutcome;
use crate::cmds::rsl::rules::ordering_rule::OrderingRuleViolation;

impl FormattedRuleViolation for OrderingRuleViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        let instruction = match &self.outcome {
            OrderingOutcome::Computed {
                required_item_order, ..
            } => {
                let ranges: Vec<_> = required_item_order
                    .iter()
                    .filter_map(|&index| self.items.get(index))
                    .map(|item| item.range.format_compact())
                    .collect();
                format!("{}: order=[{}]", self.scope, ranges.join(","))
            }
            OrderingOutcome::Failed { message, .. } => format!("{}: conflict={message}", self.scope),
        };
        format_violation(&self.location, "ordering_rule", &instruction, output_format)
    }
}
