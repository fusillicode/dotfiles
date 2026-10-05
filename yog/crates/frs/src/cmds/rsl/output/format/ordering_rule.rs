use super::FormattedRuleViolation;
use super::ViolationOutputFormat;
use super::format_violation;
use crate::cmds::rsl::rules::ordering_rule::Arrangement;
use crate::cmds::rsl::rules::ordering_rule::OrderingRuleViolation;

impl FormattedRuleViolation for OrderingRuleViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        let instruction = match &self.details {
            Arrangement::Ordered { required, .. } => {
                let ranges: Vec<_> = required
                    .iter()
                    .filter_map(|&index| self.items.get(index))
                    .map(|item| item.range.compact())
                    .collect();
                format!("{}: order=[{}]", self.scope, ranges.join(","))
            }
            Arrangement::Conflict { message, .. } => format!("{}: conflict={message}", self.scope),
        };
        format_violation(&self.location, "ordering_rule", &instruction, output_format)
    }
}
