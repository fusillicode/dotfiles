use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use muxr_core::PaneId;
use rootcause::prelude::ResultExt;

const HISTORY_FILE_MODE: u32 = 0o600;
const HISTORY_READ_BUFFER_SIZE: usize = 8192;
// A 64MiB replay cap was tried while investigating Codex resume history, but that history is app-owned UI state rather
// than muxr scrollback. Keep reattach bounded; large replay tails delay startup for every restored pane.
const HISTORY_REPLAY_LIMIT_BYTES: u64 = 4_194_304;

pub struct PaneHistory {
    file: File,
}

impl PaneHistory {
    pub fn open(path: &Path) -> rootcause::Result<(Self, Vec<u8>)> {
        let replay = self::read_tail(path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("failed to create muxr pane history directory")?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(HISTORY_FILE_MODE)
            .open(path)
            .context("failed to open muxr pane history")?;
        fs::set_permissions(path, fs::Permissions::from_mode(HISTORY_FILE_MODE))
            .context("failed to secure muxr pane history permissions")?;

        Ok((Self { file }, replay))
    }

    pub fn append(&mut self, bytes: &[u8]) -> rootcause::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }

        self.file
            .write_all(bytes)
            .context("failed to append muxr pane history")?;
        Ok(())
    }
}

pub fn pane_output_path(panes_root: &Path, pane_id: &PaneId) -> PathBuf {
    panes_root.join(pane_id.as_ref()).join("output.raw")
}

fn read_tail(path: &Path) -> rootcause::Result<Vec<u8>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("failed to open muxr pane history for replay")?,
    };
    let len = file.metadata().context("failed to inspect muxr pane history")?.len();
    let start = len.saturating_sub(HISTORY_REPLAY_LIMIT_BYTES);
    file.seek(SeekFrom::Start(start))
        .context("failed to seek muxr pane history")?;

    let mut replay = Vec::new();
    let mut buffer = [0; HISTORY_READ_BUFFER_SIZE];
    loop {
        let bytes_read = file.read(&mut buffer).context("failed to read muxr pane history")?;
        if bytes_read == 0 {
            break;
        }
        let bytes = buffer
            .get(..bytes_read)
            .ok_or_else(|| rootcause::report!("muxr pane history read exceeded buffer"))?;
        replay.extend_from_slice(bytes);
    }
    Ok(replay)
}

#[cfg(test)]
mod tests {
    use rootcause::report;

    use super::*;

    #[test]
    fn test_pane_history_open_when_file_exists_returns_replay_tail() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let path = tempdir.path().join("pane-1").join("output.raw");
        fs::create_dir_all(path.parent().ok_or_else(|| report!("expected parent"))?)?;
        fs::write(&path, b"abc").context("failed to write muxr test history")?;

        let (_history, replay) = PaneHistory::open(&path)?;

        pretty_assertions::assert_eq!(replay, b"abc".to_vec());
        Ok(())
    }

    #[test]
    fn test_pane_history_append_when_bytes_arrive_persists_output() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let path = tempdir.path().join("pane-1").join("output.raw");
        let (mut history, replay) = PaneHistory::open(&path)?;

        pretty_assertions::assert_eq!(replay, Vec::<u8>::new());
        history.append(b"abc")?;
        drop(history);

        pretty_assertions::assert_eq!(
            fs::read(&path).context("failed to read muxr test history")?,
            b"abc".to_vec()
        );
        Ok(())
    }
}
