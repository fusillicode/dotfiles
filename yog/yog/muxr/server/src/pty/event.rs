use portable_pty::ExitStatus;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug)]
pub enum PtyEvent {
    Exited,
    OutputReady,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PtyExitStatus {
    pub code: u32,
    pub signal: Option<String>,
    pub success: bool,
}

impl From<&ExitStatus> for PtyExitStatus {
    fn from(status: &ExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            signal: status.signal().map(ToOwned::to_owned),
            success: status.success(),
        }
    }
}
