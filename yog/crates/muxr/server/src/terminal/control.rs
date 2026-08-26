use smallvec::SmallVec;

const OSC_CURSOR_SHAPE_PREFIX: &[u8] = b"CursorShape=";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CursorControl {
    #[default]
    Unchanged,
    DefaultShape,
    ExplicitShape,
    Reset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AlternateScreenControl {
    EnterLegacy,
    ExitLegacy,
    InvalidatePreserved,
}

#[derive(Default)]
pub(super) struct ControlEffects {
    pub(super) alternate_screen: SmallVec<[(usize, AlternateScreenControl); 2]>,
    pub(super) cursor: CursorControl,
}

#[derive(Default)]
pub(super) struct ControlParser {
    state: CursorControlState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CursorControlState {
    #[default]
    Ground,
    Escape,
    CsiParameter(CursorParameter),
    CsiPrivateParameter {
        alternate_screen: AlternateScreenParameter,
        parameter: CursorParameter,
    },
    CsiSpace(CursorParameter),
    CsiInvalid,
    OscCursorShape(OscCursorShapeState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OscCursorShapeState {
    CommandStart,
    CommandFive,
    CommandFifty,
    Prefix(usize),
    Value,
    Valid,
    Invalid,
}

impl OscCursorShapeState {
    fn next(self, byte: u8) -> Self {
        match self {
            Self::CommandStart if byte == b'5' => Self::CommandFive,
            Self::CommandFive if byte == b'0' => Self::CommandFifty,
            Self::CommandFifty if byte == b';' => Self::Prefix(0),
            Self::Prefix(index) if OSC_CURSOR_SHAPE_PREFIX.get(index) == Some(&byte) => {
                let next = index.saturating_add(1);
                if next == OSC_CURSOR_SHAPE_PREFIX.len() {
                    Self::Value
                } else {
                    Self::Prefix(next)
                }
            }
            Self::Value if matches!(byte, b'0'..=b'2') => Self::Valid,
            Self::Valid => Self::Valid,
            Self::CommandStart
            | Self::CommandFive
            | Self::CommandFifty
            | Self::Prefix(_)
            | Self::Value
            | Self::Invalid => Self::Invalid,
        }
    }

    const fn cursor_control(self) -> CursorControl {
        if matches!(self, Self::Valid) {
            CursorControl::ExplicitShape
        } else {
            CursorControl::Unchanged
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CursorParameter {
    Empty,
    Value(u16),
    Invalid,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ControlAction {
    AlternateScreen(AlternateScreenControl),
    Cursor(CursorControl),
    Reset,
    #[default]
    Unchanged,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AlternateScreenParameter {
    #[default]
    Absent,
    Legacy,
    Native,
}

impl AlternateScreenParameter {
    const fn record(self, parameter: CursorParameter) -> Self {
        match (self, parameter) {
            (_, CursorParameter::Value(1049)) => Self::Native,
            (Self::Absent, CursorParameter::Value(47)) => Self::Legacy,
            (current, _) => current,
        }
    }
}

impl ControlParser {
    pub(super) fn process(&mut self, bytes: &[u8]) -> ControlEffects {
        let mut effects = ControlEffects::default();
        for (index, byte) in bytes.iter().enumerate() {
            match self.observe_byte(*byte) {
                ControlAction::AlternateScreen(control) => {
                    effects.alternate_screen.push((index.saturating_add(1), control));
                }
                ControlAction::Cursor(cursor) => effects.cursor = cursor,
                ControlAction::Reset => {
                    effects.cursor = CursorControl::Reset;
                    effects
                        .alternate_screen
                        .push((index.saturating_add(1), AlternateScreenControl::InvalidatePreserved));
                }
                ControlAction::Unchanged => {}
            }
        }
        effects
    }

    fn observe_byte(&mut self, byte: u8) -> ControlAction {
        match self.state {
            CursorControlState::Ground => self.observe_ground(byte),
            CursorControlState::Escape => self.observe_escape(byte),
            CursorControlState::CsiParameter(parameter) => self.observe_csi_parameter(byte, parameter),
            CursorControlState::CsiPrivateParameter {
                alternate_screen,
                parameter,
            } => self.observe_csi_private_parameter(byte, parameter, alternate_screen),
            CursorControlState::CsiSpace(parameter) => self.observe_csi_space(byte, parameter),
            CursorControlState::CsiInvalid => self.observe_csi_invalid(byte),
            CursorControlState::OscCursorShape(state) => self.observe_osc_cursor_shape(byte, state),
        }
    }

    const fn observe_ground(&mut self, byte: u8) -> ControlAction {
        self.state = match byte {
            b'\x1b' => CursorControlState::Escape,
            _ => CursorControlState::Ground,
        };
        ControlAction::Unchanged
    }

    const fn observe_escape(&mut self, byte: u8) -> ControlAction {
        match byte {
            b'\x1b' => self.state = CursorControlState::Escape,
            b'[' => self.state = CursorControlState::CsiParameter(CursorParameter::Empty),
            b']' => self.state = CursorControlState::OscCursorShape(OscCursorShapeState::CommandStart),
            b'c' => {
                self.state = CursorControlState::Ground;
                return ControlAction::Reset;
            }
            _ => self.state = CursorControlState::Ground,
        }
        ControlAction::Unchanged
    }

    fn observe_csi_parameter(&mut self, byte: u8, parameter: CursorParameter) -> ControlAction {
        self.state = match byte {
            b'0'..=b'9' => CursorControlState::CsiParameter(Self::append_cursor_parameter(parameter, byte)),
            b'?' if parameter == CursorParameter::Empty => CursorControlState::CsiPrivateParameter {
                alternate_screen: AlternateScreenParameter::Absent,
                parameter: CursorParameter::Empty,
            },
            b' ' => CursorControlState::CsiSpace(parameter),
            0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => CursorControlState::CsiParameter(parameter),
            b'\x18' | b'\x1a' | 0x40..=0x7e => CursorControlState::Ground,
            b'\x1b' => CursorControlState::Escape,
            _ => CursorControlState::CsiInvalid,
        };
        ControlAction::Unchanged
    }

    fn observe_csi_private_parameter(
        &mut self,
        byte: u8,
        parameter: CursorParameter,
        alternate_screen: AlternateScreenParameter,
    ) -> ControlAction {
        match byte {
            b'0'..=b'9' => {
                self.state = CursorControlState::CsiPrivateParameter {
                    alternate_screen,
                    parameter: Self::append_cursor_parameter(parameter, byte),
                };
                ControlAction::Unchanged
            }
            b';' => {
                self.state = CursorControlState::CsiPrivateParameter {
                    alternate_screen: alternate_screen.record(parameter),
                    parameter: CursorParameter::Empty,
                };
                ControlAction::Unchanged
            }
            0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => {
                self.state = CursorControlState::CsiPrivateParameter {
                    alternate_screen,
                    parameter,
                };
                ControlAction::Unchanged
            }
            b'h' | b'l' => {
                self.state = CursorControlState::Ground;
                match (alternate_screen.record(parameter), byte) {
                    (AlternateScreenParameter::Legacy, b'h') => {
                        ControlAction::AlternateScreen(AlternateScreenControl::EnterLegacy)
                    }
                    (AlternateScreenParameter::Legacy, b'l') => {
                        ControlAction::AlternateScreen(AlternateScreenControl::ExitLegacy)
                    }
                    (AlternateScreenParameter::Native, b'h' | b'l') => {
                        ControlAction::AlternateScreen(AlternateScreenControl::InvalidatePreserved)
                    }
                    (AlternateScreenParameter::Absent, _) => ControlAction::Unchanged,
                    (AlternateScreenParameter::Legacy | AlternateScreenParameter::Native, _) => {
                        ControlAction::Unchanged
                    }
                }
            }
            b'\x18' | b'\x1a' | 0x40..=0x7e => {
                self.state = CursorControlState::Ground;
                ControlAction::Unchanged
            }
            b'\x1b' => {
                self.state = CursorControlState::Escape;
                ControlAction::Unchanged
            }
            _ => {
                self.state = CursorControlState::CsiInvalid;
                ControlAction::Unchanged
            }
        }
    }

    const fn observe_csi_space(&mut self, byte: u8, parameter: CursorParameter) -> ControlAction {
        match byte {
            b'q' => {
                self.state = CursorControlState::Ground;
                match parameter {
                    CursorParameter::Empty | CursorParameter::Value(0) => {
                        ControlAction::Cursor(CursorControl::DefaultShape)
                    }
                    CursorParameter::Value(1..=6) => ControlAction::Cursor(CursorControl::ExplicitShape),
                    CursorParameter::Value(_) | CursorParameter::Invalid => ControlAction::Unchanged,
                }
            }
            0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => {
                self.state = CursorControlState::CsiSpace(parameter);
                ControlAction::Unchanged
            }
            b'\x18' | b'\x1a' | 0x40..=0x7e => {
                self.state = CursorControlState::Ground;
                ControlAction::Unchanged
            }
            b'\x1b' => {
                self.state = CursorControlState::Escape;
                ControlAction::Unchanged
            }
            _ => {
                self.state = CursorControlState::CsiInvalid;
                ControlAction::Unchanged
            }
        }
    }

    const fn observe_csi_invalid(&mut self, byte: u8) -> ControlAction {
        self.state = match byte {
            b'\x18' | b'\x1a' | 0x40..=0x7e => CursorControlState::Ground,
            b'\x1b' => CursorControlState::Escape,
            _ => CursorControlState::CsiInvalid,
        };
        ControlAction::Unchanged
    }

    fn observe_osc_cursor_shape(&mut self, byte: u8, state: OscCursorShapeState) -> ControlAction {
        match byte {
            b'\x07' | b'\x18' | b'\x1a' => {
                self.state = CursorControlState::Ground;
                ControlAction::Cursor(state.cursor_control())
            }
            b'\x1b' => {
                self.state = CursorControlState::Escape;
                ControlAction::Cursor(state.cursor_control())
            }
            0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f => {
                self.state = CursorControlState::OscCursorShape(state);
                ControlAction::Unchanged
            }
            _ => {
                self.state = CursorControlState::OscCursorShape(state.next(byte));
                ControlAction::Unchanged
            }
        }
    }

    fn append_cursor_parameter(parameter: CursorParameter, byte: u8) -> CursorParameter {
        let digit = u16::from(byte.saturating_sub(b'0'));
        match parameter {
            CursorParameter::Empty => CursorParameter::Value(digit),
            CursorParameter::Value(value) => value
                .checked_mul(10)
                .and_then(|value| value.checked_add(digit))
                .map_or(CursorParameter::Invalid, CursorParameter::Value),
            CursorParameter::Invalid => CursorParameter::Invalid,
        }
    }
}
