use std::path::Path;
use std::path::PathBuf;

use muxr_core::SessionName;
use rootcause::prelude::ResultExt;
use rootcause::report;

const SERVER_EXECUTABLE: &str = "muxr-server";

pub fn run(session: &SessionName, external_layout: Option<PathBuf>) -> rootcause::Result<()> {
    let current_exe = std::env::current_exe().context("failed to resolve muxr executable")?;
    let server_executable = server_executable_next_to(&current_exe)?;
    let external_layout = match external_layout {
        Some(path) if path.is_relative() => Some(
            std::env::current_dir()
                .context("failed to resolve muxr cwd")?
                .join(path),
        ),
        Some(path) => Some(path),
        None => None,
    };
    muxr_client::start(session, &server_executable, external_layout.as_deref())?;
    Ok(())
}

pub(super) fn server_executable_next_to(current_exe: &Path) -> rootcause::Result<PathBuf> {
    let Some(parent) = current_exe.parent().filter(|parent| !parent.as_os_str().is_empty()) else {
        return Err(
            report!("muxr executable has no parent dir").attach(format!("executable={}", current_exe.display()))
        );
    };
    // Keep the attached client and long-lived server as separate processes: `muxr` can link picker/UI-only CLI deps,
    // while `muxr-server` keeps session state, PTYs, and scrollback memory attributable to the server runtime alone.
    Ok(parent.join(SERVER_EXECUTABLE))
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_server_executable_next_to_returns_sibling_without_checking_existence() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let muxr = tempdir.path().join("muxr");
        let runner = tempdir.path().join(SERVER_EXECUTABLE);

        assert_that!(server_executable_next_to(&muxr)?, eq(runner));
        Ok(())
    }
}
