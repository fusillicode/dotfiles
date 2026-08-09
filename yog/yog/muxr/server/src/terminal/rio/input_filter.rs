use smallvec::SmallVec;

const DCS_HEADER_LIMIT: usize = 64;
const CSI_SEQUENCE_LIMIT: usize = 256;
const ESCAPE: u8 = b'\x1b';
const OSC_COMMAND_LIMIT: usize = 4;
const OSC_PASSTHROUGH_LIMIT: usize = 8 * 1024;
const XTGETTCAP_PAYLOAD_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum InputFilterState {
    #[default]
    Ground,
    Escape,
    Csi,
    DiscardCsi,
    DcsHeader,
    OscPrefix,
    BufferedOsc {
        remaining: usize,
    },
    BufferedOscEscape {
        remaining: usize,
    },
    DiscardString(StringTerminator),
    DiscardStringEscape(StringTerminator),
    PassthroughString {
        limit: StringPassthroughLimit,
        terminator: StringTerminator,
    },
    PassthroughStringEscape {
        limit: StringPassthroughLimit,
        terminator: StringTerminator,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StringTerminator {
    BellOrStringTerminator,
    StringTerminator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StringPassthroughLimit {
    Remaining(usize),
    Unlimited,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OscCommandHandling {
    Discard,
    Passthrough,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Utf8State {
    Continuation {
        remaining: u8,
    },
    FirstContinuation {
        lower: u8,
        remaining_after: u8,
        upper: u8,
    },
    #[default]
    Ground,
}

#[derive(Default)]
struct C1Normalizer {
    utf8_state: Utf8State,
}

impl C1Normalizer {
    fn normalize(&mut self, bytes: &[u8]) -> Option<Vec<u8>> {
        let mut normalized: Option<Vec<u8>> = None;
        for (index, byte) in bytes.iter().enumerate() {
            let Some(mapped) = self.observe_byte(*byte) else {
                if let Some(normalized) = normalized.as_mut() {
                    normalized.push(*byte);
                }
                continue;
            };

            let normalized = normalized.get_or_insert_with(|| {
                let mut normalized = Vec::with_capacity(bytes.len().saturating_add(2));
                if let Some(prefix) = bytes.get(..index) {
                    normalized.extend_from_slice(prefix);
                }
                normalized
            });
            normalized.push(ESCAPE);
            normalized.push(mapped);
        }
        normalized
    }

    fn observe_byte(&mut self, byte: u8) -> Option<u8> {
        loop {
            match self.utf8_state {
                Utf8State::Continuation { remaining } if (0x80..=0xbf).contains(&byte) => {
                    self.utf8_state = if remaining > 1 {
                        Utf8State::Continuation {
                            remaining: remaining.saturating_sub(1),
                        }
                    } else {
                        Utf8State::Ground
                    };
                    return None;
                }
                Utf8State::FirstContinuation {
                    lower,
                    remaining_after,
                    upper,
                } if (lower..=upper).contains(&byte) => {
                    self.utf8_state = Utf8State::Continuation {
                        remaining: remaining_after,
                    };
                    return None;
                }
                Utf8State::Continuation { .. } | Utf8State::FirstContinuation { .. } => {
                    self.utf8_state = Utf8State::Ground;
                }
                Utf8State::Ground => {
                    self.utf8_state = match byte {
                        0xc2..=0xdf => Utf8State::Continuation { remaining: 1 },
                        0xe0 => Utf8State::FirstContinuation {
                            lower: 0xa0,
                            remaining_after: 1,
                            upper: 0xbf,
                        },
                        0xe1..=0xec | 0xee..=0xef => Utf8State::FirstContinuation {
                            lower: 0x80,
                            remaining_after: 1,
                            upper: 0xbf,
                        },
                        0xed => Utf8State::FirstContinuation {
                            lower: 0x80,
                            remaining_after: 1,
                            upper: 0x9f,
                        },
                        0xf0 => Utf8State::FirstContinuation {
                            lower: 0x90,
                            remaining_after: 2,
                            upper: 0xbf,
                        },
                        0xf1..=0xf3 => Utf8State::FirstContinuation {
                            lower: 0x80,
                            remaining_after: 2,
                            upper: 0xbf,
                        },
                        0xf4 => Utf8State::FirstContinuation {
                            lower: 0x80,
                            remaining_after: 2,
                            upper: 0x8f,
                        },
                        _ => Utf8State::Ground,
                    };
                    return (0x80..=0x9f).contains(&byte).then_some(byte.saturating_sub(0x40));
                }
            }
        }
    }
}

pub(in crate::terminal) enum FilteredInput<'a> {
    Original(&'a [u8]),
    Filtered(&'a [u8]),
}

impl AsRef<[u8]> for FilteredInput<'_> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Original(bytes) | Self::Filtered(bytes) => bytes,
        }
    }
}

#[derive(Default)]
pub(in crate::terminal) struct RioInputFilter {
    c1_normalizer: C1Normalizer,
    output: Vec<u8>,
    pending: SmallVec<[u8; DCS_HEADER_LIMIT]>,
    state: InputFilterState,
}

impl RioInputFilter {
    pub(in crate::terminal) fn process<'a>(&'a mut self, bytes: &'a [u8]) -> FilteredInput<'a> {
        let normalized = self.c1_normalizer.normalize(bytes);
        if normalized.is_none() && self.state == InputFilterState::Ground && !bytes.contains(&ESCAPE) {
            return FilteredInput::Original(bytes);
        }
        let bytes = normalized.as_deref().unwrap_or(bytes);

        self.output.clear();
        for byte in bytes {
            self.process_byte(*byte);
        }
        FilteredInput::Filtered(&self.output)
    }

    fn process_byte(&mut self, byte: u8) {
        match self.state {
            InputFilterState::Ground => self.process_ground(byte),
            InputFilterState::Escape => self.process_escape(byte),
            InputFilterState::Csi => self.process_csi(byte),
            InputFilterState::DiscardCsi => self.process_discard_csi(byte),
            InputFilterState::DcsHeader => self.process_dcs_header(byte),
            InputFilterState::OscPrefix => self.process_osc_prefix(byte),
            InputFilterState::BufferedOsc { remaining } => self.process_buffered_osc(byte, remaining),
            InputFilterState::BufferedOscEscape { remaining } => {
                self.process_buffered_osc_escape(byte, remaining);
            }
            InputFilterState::DiscardString(terminator) => self.process_discard_string(byte, terminator),
            InputFilterState::DiscardStringEscape(terminator) => {
                self.process_discard_string_escape(byte, terminator);
            }
            InputFilterState::PassthroughString { limit, terminator } => {
                self.process_passthrough_string(byte, terminator, limit);
            }
            InputFilterState::PassthroughStringEscape { limit, terminator } => {
                self.process_passthrough_string_escape(byte, terminator, limit);
            }
        }
    }

    fn process_ground(&mut self, byte: u8) {
        if byte == ESCAPE {
            self.pending.clear();
            self.pending.push(byte);
            self.state = InputFilterState::Escape;
        } else {
            self.output.push(byte);
        }
    }

    fn process_escape(&mut self, byte: u8) {
        match byte {
            b'_' => {
                self.pending.clear();
                self.state = InputFilterState::DiscardString(StringTerminator::BellOrStringTerminator);
            }
            b'P' => {
                self.pending.push(byte);
                self.state = InputFilterState::DcsHeader;
            }
            b'[' => {
                self.pending.push(byte);
                self.state = InputFilterState::Csi;
            }
            b']' => {
                self.pending.push(byte);
                self.state = InputFilterState::OscPrefix;
            }
            ESCAPE => {
                self.flush_pending();
                self.pending.push(byte);
            }
            _ => {
                self.pending.push(byte);
                self.flush_pending();
                self.state = InputFilterState::Ground;
            }
        }
    }

    fn process_csi(&mut self, byte: u8) {
        self.pending.push(byte);
        if byte == ESCAPE {
            let _escape = self.pending.pop();
            self.flush_pending();
            self.pending.push(ESCAPE);
            self.state = InputFilterState::Escape;
            return;
        }
        if self::is_string_cancel(byte) {
            self.flush_pending();
            self.state = InputFilterState::Ground;
            return;
        }
        if (0x40..=0x7e).contains(&byte) {
            if !self.strip_sync_update_mode() {
                self.flush_pending();
            }
            self.pending.clear();
            self.state = InputFilterState::Ground;
            return;
        }

        if self.pending.len() >= CSI_SEQUENCE_LIMIT {
            self.pending.clear();
            self.state = InputFilterState::DiscardCsi;
        }
    }

    fn process_discard_csi(&mut self, byte: u8) {
        if byte == ESCAPE {
            self.pending.clear();
            self.pending.push(ESCAPE);
            self.state = InputFilterState::Escape;
        } else if self::is_string_cancel(byte) || (0x40..=0x7e).contains(&byte) {
            self.state = InputFilterState::Ground;
        }
    }

    fn strip_sync_update_mode(&mut self) -> bool {
        let Some(sequence) = self.pending.strip_prefix(b"\x1b[?") else {
            return false;
        };
        let Some((final_byte, parameters)) = sequence.split_last() else {
            return false;
        };
        if !matches!(*final_byte, b'h' | b'l')
            || !parameters
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b';' | b':'))
        {
            return false;
        }

        let parameters = parameters.split(|byte| *byte == b';').collect::<SmallVec<[&[u8]; 8]>>();
        if !parameters
            .iter()
            .any(|parameter| self::is_sync_update_parameter(parameter))
        {
            return false;
        }

        let mut kept = parameters
            .into_iter()
            .filter(|parameter| !self::is_sync_update_parameter(parameter))
            .peekable();
        if kept.peek().is_none() {
            return true;
        }
        self.output.extend_from_slice(b"\x1b[?");
        for (index, parameter) in kept.enumerate() {
            if index > 0 {
                self.output.push(b';');
            }
            self.output.extend_from_slice(parameter);
        }
        self.output.push(*final_byte);
        true
    }

    fn process_dcs_header(&mut self, byte: u8) {
        if self::is_string_cancel(byte) {
            self.pending.push(byte);
            self.flush_pending();
            self.state = InputFilterState::Ground;
            return;
        }
        if self::is_dcs_header_ignored(byte) {
            return;
        }

        self.pending.push(byte);
        if byte == ESCAPE {
            self.begin_passthrough(
                byte,
                StringTerminator::StringTerminator,
                StringPassthroughLimit::Unlimited,
            );
            return;
        }

        if (0x40..=0x7e).contains(&byte) {
            if self.is_sixel_header() {
                self.pending.clear();
                self.state = InputFilterState::DiscardString(StringTerminator::StringTerminator);
            } else {
                let limit = if self.is_xtgettcap_header() {
                    StringPassthroughLimit::Remaining(XTGETTCAP_PAYLOAD_LIMIT)
                } else {
                    StringPassthroughLimit::Unlimited
                };
                self.begin_passthrough(byte, StringTerminator::StringTerminator, limit);
            }
            return;
        }

        if self.pending.len() >= DCS_HEADER_LIMIT {
            self.flush_pending();
            self.state = InputFilterState::PassthroughString {
                limit: StringPassthroughLimit::Unlimited,
                terminator: StringTerminator::StringTerminator,
            };
        }
    }

    fn process_osc_prefix(&mut self, byte: u8) {
        self.pending.push(byte);
        if matches!(byte, b';' | ESCAPE)
            || self::is_string_cancel(byte)
            || self::is_bell_terminator(byte, StringTerminator::BellOrStringTerminator)
        {
            if self.osc_command_handling() == OscCommandHandling::Passthrough {
                self.begin_buffered_osc(byte);
            } else {
                self.begin_discard(byte, StringTerminator::BellOrStringTerminator);
            }
            return;
        }

        let command_len = self.pending.len().saturating_sub(2);
        if byte.is_ascii_digit() && command_len <= OSC_COMMAND_LIMIT {
            return;
        }

        self.begin_discard(byte, StringTerminator::BellOrStringTerminator);
    }

    fn osc_command_handling(&self) -> OscCommandHandling {
        let Some(command_end) = self.pending.len().checked_sub(1) else {
            return OscCommandHandling::Discard;
        };
        let Some(command) = self.pending.get(2..command_end) else {
            return OscCommandHandling::Discard;
        };
        match command {
            b"0" | b"2" | b"8" | b"50" => OscCommandHandling::Passthrough,
            _ => OscCommandHandling::Discard,
        }
    }

    fn begin_buffered_osc(&mut self, last_byte: u8) {
        if self::is_string_cancel(last_byte) {
            self.pending.clear();
            self.state = InputFilterState::Ground;
        } else if self::is_bell_terminator(last_byte, StringTerminator::BellOrStringTerminator) {
            self.flush_pending();
            self.state = InputFilterState::Ground;
        } else if last_byte == ESCAPE {
            self.state = InputFilterState::BufferedOscEscape {
                remaining: OSC_PASSTHROUGH_LIMIT,
            };
        } else {
            self.state = InputFilterState::BufferedOsc {
                remaining: OSC_PASSTHROUGH_LIMIT,
            };
        }
    }

    fn process_buffered_osc(&mut self, byte: u8, remaining: usize) {
        if self::is_string_cancel(byte) {
            self.pending.clear();
            self.state = InputFilterState::Ground;
            return;
        }
        if self::is_bell_terminator(byte, StringTerminator::BellOrStringTerminator) {
            self.pending.push(byte);
            self.flush_pending();
            self.state = InputFilterState::Ground;
            return;
        }
        if byte == ESCAPE {
            self.pending.push(byte);
            self.state = InputFilterState::BufferedOscEscape { remaining };
            return;
        }
        if remaining == 0 {
            self.pending.extend_from_slice(b"\x1b\\");
            self.flush_pending();
            self.state = InputFilterState::DiscardString(StringTerminator::BellOrStringTerminator);
            return;
        }

        self.pending.push(byte);
        self.state = InputFilterState::BufferedOsc {
            remaining: remaining.saturating_sub(1),
        };
    }

    fn process_buffered_osc_escape(&mut self, byte: u8, _remaining: usize) {
        if byte == b'\\' {
            self.pending.push(byte);
            self.flush_pending();
            self.state = InputFilterState::Ground;
            return;
        }

        self.pending.push(b'\\');
        self.flush_pending();
        self.pending.push(ESCAPE);
        self.state = InputFilterState::Escape;
        self.process_escape(byte);
    }

    fn begin_discard(&mut self, last_byte: u8, terminator: StringTerminator) {
        self.pending.clear();
        if self::is_string_cancel(last_byte) || self::is_bell_terminator(last_byte, terminator) {
            self.state = InputFilterState::Ground;
        } else if last_byte == ESCAPE {
            self.pending.push(ESCAPE);
            self.state = InputFilterState::DiscardStringEscape(terminator);
        } else {
            self.state = InputFilterState::DiscardString(terminator);
        }
    }

    fn process_discard_string(&mut self, byte: u8, terminator: StringTerminator) {
        if self::is_string_cancel(byte) || self::is_bell_terminator(byte, terminator) {
            self.state = InputFilterState::Ground;
        } else if byte == ESCAPE {
            self.pending.clear();
            self.pending.push(byte);
            self.state = InputFilterState::DiscardStringEscape(terminator);
        }
    }

    fn process_discard_string_escape(&mut self, byte: u8, _terminator: StringTerminator) {
        if byte == b'\\' {
            self.pending.clear();
            self.state = InputFilterState::Ground;
            return;
        }

        self.state = InputFilterState::Escape;
        self.process_escape(byte);
    }

    fn process_passthrough_string(&mut self, byte: u8, terminator: StringTerminator, limit: StringPassthroughLimit) {
        if byte == ESCAPE {
            self.pending.clear();
            self.pending.push(byte);
            self.state = InputFilterState::PassthroughStringEscape { limit, terminator };
            return;
        }

        if self::is_string_cancel(byte) || self::is_bell_terminator(byte, terminator) {
            self.output.push(byte);
            self.state = InputFilterState::Ground;
            return;
        }

        match limit {
            StringPassthroughLimit::Remaining(0) => {
                self.output.extend_from_slice(b"\x1b\\");
                self.state = InputFilterState::DiscardString(terminator);
            }
            StringPassthroughLimit::Remaining(remaining) => {
                self.output.push(byte);
                self.state = InputFilterState::PassthroughString {
                    limit: StringPassthroughLimit::Remaining(remaining.saturating_sub(1)),
                    terminator,
                };
            }
            StringPassthroughLimit::Unlimited => {
                self.output.push(byte);
                self.state = InputFilterState::PassthroughString { limit, terminator };
            }
        }
    }

    fn process_passthrough_string_escape(
        &mut self,
        byte: u8,
        _terminator: StringTerminator,
        _limit: StringPassthroughLimit,
    ) {
        if byte == b'\\' {
            self.pending.push(byte);
            self.flush_pending();
            self.state = InputFilterState::Ground;
            return;
        }

        self.pending.clear();
        self.output.extend_from_slice(b"\x1b\\");
        self.pending.push(ESCAPE);
        self.state = InputFilterState::Escape;
        self.process_escape(byte);
    }

    fn begin_passthrough(&mut self, last_byte: u8, terminator: StringTerminator, limit: StringPassthroughLimit) {
        if last_byte == ESCAPE {
            let _last = self.pending.pop();
            self.flush_pending();
            self.pending.push(ESCAPE);
            self.state = InputFilterState::PassthroughStringEscape { limit, terminator };
            return;
        }

        self.flush_pending();
        self.state = if self::is_string_cancel(last_byte) || self::is_bell_terminator(last_byte, terminator) {
            InputFilterState::Ground
        } else {
            InputFilterState::PassthroughString { limit, terminator }
        };
    }

    fn flush_pending(&mut self) {
        self.output.extend_from_slice(&self.pending);
        self.pending.clear();
    }

    fn is_sixel_header(&self) -> bool {
        let Some(header) = self.pending.get(2..) else {
            return false;
        };
        let Some((final_byte, parameters)) = header.split_last() else {
            return false;
        };
        *final_byte == b'q' && parameters.iter().all(|byte| matches!(byte, 0x30..=0x3f))
    }

    fn is_xtgettcap_header(&self) -> bool {
        let Some(header) = self.pending.get(2..) else {
            return false;
        };
        let Some((final_byte, header)) = header.split_last() else {
            return false;
        };
        let Some((intermediate, parameters)) = header.split_last() else {
            return false;
        };
        *final_byte == b'q' && *intermediate == b'+' && parameters.iter().all(|byte| matches!(byte, 0x30..=0x3f))
    }
}

const fn is_bell_terminator(byte: u8, terminator: StringTerminator) -> bool {
    byte == b'\x07' && matches!(terminator, StringTerminator::BellOrStringTerminator)
}

const fn is_string_cancel(byte: u8) -> bool {
    matches!(byte, b'\x18' | b'\x1a')
}

const fn is_dcs_header_ignored(byte: u8) -> bool {
    matches!(byte, 0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f)
}

fn is_sync_update_parameter(parameter: &[u8]) -> bool {
    let Some(primary) = parameter.split(|byte| *byte == b':').next() else {
        return false;
    };
    if primary.is_empty() {
        return false;
    }
    primary.iter().try_fold(0_u16, |value, byte| {
        value.checked_mul(10)?.checked_add(u16::from(byte.saturating_sub(b'0')))
    }) == Some(2026)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rio_input_filter_when_kitty_graphics_is_split_discards_payload() {
        let mut filter = RioInputFilter::default();

        let first = filter.process(b"before\x1b_Ga=T,f=24,s=1,v=1,m=1;AAAA");
        std::assert_eq!(first.as_ref(), b"before");
        let second = filter.process(b"BBBB\x1b\\after");
        std::assert_eq!(second.as_ref(), b"after");
    }

    #[test]
    fn test_rio_input_filter_when_kitty_graphics_uses_bell_discards_payload() {
        let mut filter = RioInputFilter::default();

        let filtered = filter.process(b"before\x1b_Ga=T;AAAA\x07after");

        std::assert_eq!(filtered.as_ref(), b"beforeafter");
    }

    #[test]
    fn test_rio_input_filter_when_kitty_graphics_is_canceled_resumes_output() {
        let mut filter = RioInputFilter::default();

        let filtered = filter.process(b"before\x1b_Ga=T;AAAA\x18after");

        std::assert_eq!(filtered.as_ref(), b"beforeafter");
    }

    #[test]
    fn test_rio_input_filter_when_sixel_graphics_is_split_discards_payload() {
        let mut filter = RioInputFilter::default();

        let first = filter.process(b"before\x1bP0;0;0q~~~~");
        std::assert_eq!(first.as_ref(), b"before");
        let second = filter.process(b"~~~~\x1b\\after");
        std::assert_eq!(second.as_ref(), b"after");
    }

    #[test]
    fn test_rio_input_filter_when_sixel_uses_c1_string_terminator_resumes_output() {
        let mut filter = RioInputFilter::default();

        let filtered = filter.process(b"before\x1bP0;0;0q~~~~\x9cafter");

        std::assert_eq!(filtered.as_ref(), b"beforeafter");
    }

    #[test]
    fn test_rio_input_filter_when_iterm_metadata_is_split_discards_payload() {
        let mut filter = RioInputFilter::default();

        let first = filter.process(b"before\x1b]133");
        std::assert_eq!(first.as_ref(), b"before");
        let second = filter.process(b"7;SetUserVar=key=dmFsdWU=\x07after");
        std::assert_eq!(second.as_ref(), b"after");
    }

    #[test]
    fn test_rio_input_filter_when_control_strings_are_supported_preserves_bytes() {
        let mut filter = RioInputFilter::default();

        let osc = filter.process(b"\x1b]2;title\x07");
        std::assert_eq!(osc.as_ref(), b"\x1b]2;title\x07");
        let hyperlink = filter.process(b"\x1b]8;;https://example.com\x1b\\");
        std::assert_eq!(hyperlink.as_ref(), b"\x1b]8;;https://example.com\x1b\\");
        let dcs = filter.process(b"\x1bP$qm\x1b\\");
        std::assert_eq!(dcs.as_ref(), b"\x1bP$qm\x1b\\");
    }

    #[test]
    fn test_rio_input_filter_when_supported_osc_uses_c1_string_terminator_normalizes_terminator() {
        let mut filter = RioInputFilter::default();

        let filtered = filter.process(b"\x1b]2;title\x9cafter");

        std::assert_eq!(filtered.as_ref(), b"\x1b]2;title\x1b\\after");
    }

    #[test]
    fn test_rio_input_filter_when_utf8_contains_c1_continuation_preserves_string_text() {
        let mut filter = RioInputFilter::default();

        let first = filter.process(b"\x1b]2;\xc3");
        std::assert!(first.as_ref().is_empty());
        let second = filter.process(b"\x9c\x9cafter");

        std::assert_eq!(second.as_ref(), b"\x1b]2;\xc3\x9c\x1b\\after");
    }

    #[test]
    fn test_rio_input_filter_when_c1_apc_contains_graphics_discards_payload() {
        let mut filter = RioInputFilter::default();

        let filtered = filter.process(b"before\x9fGa=T;AAAA\x9cafter");

        std::assert_eq!(filtered.as_ref(), b"beforeafter");
    }

    #[test]
    fn test_rio_input_filter_when_unsupported_osc_has_large_payload_discards_payload() {
        let mut filter = RioInputFilter::default();
        let mut input = b"before\x1b]52;c;".to_vec();
        input.extend(std::iter::repeat_n(b'A', 16 * 1024));
        input.extend_from_slice(b"\x07after");

        let filtered = filter.process(&input);

        std::assert_eq!(filtered.as_ref(), b"beforeafter");
    }

    #[test]
    fn test_rio_input_filter_when_supported_osc_exceeds_limit_bounds_forwarded_payload() {
        let mut filter = RioInputFilter::default();
        let mut input = b"\x1b]2;".to_vec();
        input.extend(std::iter::repeat_n(b'A', 8 * 1024 + 1));

        let filtered = filter.process(&input);

        std::assert_eq!(filtered.as_ref().len(), b"\x1b]2;".len() + 8 * 1024 + b"\x1b\\".len());
        std::assert!(filtered.as_ref().ends_with(b"\x1b\\"));
        let resumed = filter.process(b"ignored\x07after");
        std::assert_eq!(resumed.as_ref(), b"after");
    }

    #[test]
    fn test_rio_input_filter_when_sync_update_is_split_processes_payload_immediately() {
        let mut filter = RioInputFilter::default();

        let first = filter.process(b"before\x1b[?20");
        std::assert_eq!(first.as_ref(), b"before");
        let second = filter.process(b"26hpayload\x1b[?2026lafter");
        std::assert_eq!(second.as_ref(), b"payloadafter");
    }

    #[test]
    fn test_rio_input_filter_when_sync_update_uses_c1_csi_processes_payload_immediately() {
        let mut filter = RioInputFilter::default();

        let filtered = filter.process(b"before\x9b?2026hpayload\x9b?2026lafter");

        std::assert_eq!(filtered.as_ref(), b"beforepayloadafter");
    }

    #[test]
    fn test_rio_input_filter_when_sync_update_is_grouped_preserves_other_modes() {
        let mut filter = RioInputFilter::default();

        let filtered = filter.process(b"\x1b[?1;02026:0;2004hpayload\x1b[?2026;1l");

        std::assert_eq!(filtered.as_ref(), b"\x1b[?1;2004hpayload\x1b[?1l");
    }

    #[test]
    fn test_rio_input_filter_when_regular_csi_is_split_preserves_bytes() {
        let mut filter = RioInputFilter::default();

        let first = filter.process(b"before\x1b[?20");
        std::assert_eq!(first.as_ref(), b"before");
        let second = filter.process(b"04hafter");
        std::assert_eq!(second.as_ref(), b"\x1b[?2004hafter");
    }

    #[test]
    fn test_rio_input_filter_when_xtgettcap_exceeds_limit_bounds_forwarded_payload() {
        let mut filter = RioInputFilter::default();
        let mut input = b"\x1bP+q".to_vec();
        input.extend(std::iter::repeat_n(b'A', XTGETTCAP_PAYLOAD_LIMIT.saturating_add(1)));

        let filtered = filter.process(&input);

        std::assert_eq!(
            filtered.as_ref().len(),
            b"\x1bP+q"
                .len()
                .saturating_add(XTGETTCAP_PAYLOAD_LIMIT)
                .saturating_add(2)
        );
        std::assert!(filtered.as_ref().ends_with(b"\x1b\\"));
        let resumed = filter.process(b"ignored\x1b\\after");
        std::assert_eq!(resumed.as_ref(), b"after");
    }

    #[test]
    fn test_rio_input_filter_when_parameterized_xtgettcap_exceeds_limit_bounds_forwarded_payload() {
        let mut filter = RioInputFilter::default();
        let mut input = b"\x1bP1+q".to_vec();
        input.extend(std::iter::repeat_n(b'A', XTGETTCAP_PAYLOAD_LIMIT.saturating_add(1)));

        let filtered = filter.process(&input);

        std::assert!(filtered.as_ref().ends_with(b"\x1b\\"));
        std::assert_eq!(
            filtered.as_ref().len(),
            b"\x1bP1+q"
                .len()
                .saturating_add(XTGETTCAP_PAYLOAD_LIMIT)
                .saturating_add(2)
        );
    }

    #[rstest::rstest]
    #[case::null(b'\x00')]
    #[case::bell(b'\x07')]
    #[case::delete(b'\x7f')]
    fn test_rio_input_filter_when_split_xtgettcap_header_contains_ignored_control_bounds_forwarded_payload(
        #[case] ignored: u8,
    ) {
        let mut filter = RioInputFilter::default();
        let mut header = b"\x1bP+".to_vec();
        header.push(ignored);

        let first = filter.process(&header);
        std::assert!(first.as_ref().is_empty());
        let mut payload = b"q".to_vec();
        payload.extend(std::iter::repeat_n(b'A', XTGETTCAP_PAYLOAD_LIMIT.saturating_add(1)));
        let second = filter.process(&payload);

        std::assert_eq!(
            second.as_ref().len(),
            b"\x1bP+q"
                .len()
                .saturating_add(XTGETTCAP_PAYLOAD_LIMIT)
                .saturating_add(2)
        );
        std::assert!(second.as_ref().ends_with(b"\x1b\\"));
        let resumed = filter.process(b"ignored\x1b\\after");
        std::assert_eq!(resumed.as_ref(), b"after");
    }

    #[rstest::rstest]
    #[case::can(b'\x18')]
    #[case::sub(b'\x1a')]
    fn test_rio_input_filter_when_split_dcs_header_is_canceled_resumes_ground_output(#[case] cancel: u8) {
        let mut filter = RioInputFilter::default();

        let first = filter.process(b"\x1bP+");
        std::assert!(first.as_ref().is_empty());
        let cancel_input = [cancel];
        let canceled = filter.process(&cancel_input);
        let mut expected = b"\x1bP+".to_vec();
        expected.push(cancel);
        std::assert_eq!(canceled.as_ref(), expected.as_slice());
        let resumed = filter.process(b"after");
        std::assert_eq!(resumed.as_ref(), b"after");
    }

    #[test]
    fn test_rio_input_filter_when_arbitrary_apc_is_split_discards_payload() {
        let mut filter = RioInputFilter::default();

        let first = filter.process(b"before\x1b_Xignored");
        std::assert_eq!(first.as_ref(), b"before");
        let payload = vec![b'x'; 64 * 1024];
        let middle = filter.process(&payload);
        std::assert!(middle.as_ref().is_empty());
        let last = filter.process(b"\x1b\\after");
        std::assert_eq!(last.as_ref(), b"after");
    }

    #[test]
    fn test_rio_input_filter_when_supported_string_precedes_kitty_graphics_preserves_only_supported_string() {
        let mut filter = RioInputFilter::default();

        let filtered = filter.process(b"\x1b]2;title\x1b_Ga=T;AAAA\x1b\\after");

        std::assert_eq!(filtered.as_ref(), b"\x1b]2;title\x1b\\after");
    }
}
