use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use muxr_config::ScrollbackDumpStyle;
use muxr_config::ScrollbackEditorConfig;
use muxr_core::PaneId;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::client::session::ClientSessionState;
use crate::history::pane_output_path;
use crate::keyboard_input::ClientCmd;
use crate::keyboard_input::ServerInputMode;
use crate::pane::fullscreen::PaneFullscreen;
use crate::pane::runtime::PaneRuntimes;
use crate::pty::ShellCmd;
use crate::server::ServerConfig;
use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::PaneState;
use crate::state::PaneTree;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;
use crate::terminal::TerminalFocusEvent;

const SCROLLBACK_EDITOR_TITLE: &str = "scrollback";

#[derive(Debug)]
struct OpenScrollbackEditor {
    state: ScrollbackEditorState,
}

#[derive(Debug)]
pub struct ScrollbackEditorState {
    dump_path: PathBuf,
    editor_pane_id: PaneId,
    original_fullscreen: PaneFullscreen,
    original_layout: SessionLayout,
}

impl ScrollbackEditorState {
    const fn editor_pane_id(&self) -> PaneId {
        self.editor_pane_id
    }
}

#[derive(Debug)]
pub enum ScrollbackEditorOpenClientOutcome {
    AlreadyOpen,
    Opened {
        editor: ScrollbackEditorState,
        editor_pane_id: PaneId,
        previous_pane: PaneId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollbackEditorRestoreOutcome {
    pub editor_pane_id: Option<PaneId>,
}

impl ScrollbackEditorRestoreOutcome {
    const fn unchanged() -> Self {
        Self { editor_pane_id: None }
    }

    pub const fn restored(self) -> bool {
        self.editor_pane_id.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollbackEditorCmdAction {
    Ignore,
    Restore,
    Run(ClientCmd),
}

pub const fn cmd_action(cmd: ClientCmd, editor_active: bool) -> ScrollbackEditorCmdAction {
    if !editor_active {
        return ScrollbackEditorCmdAction::Run(cmd);
    }
    match cmd {
        ClientCmd::ClosePane => ScrollbackEditorCmdAction::Restore,
        ClientCmd::OpenScrollbackEditor
        | ClientCmd::SplitPane(_)
        | ClientCmd::FocusPane(_)
        | ClientCmd::EnterResizeMode
        | ClientCmd::ExitMode
        | ClientCmd::ResizePane(_)
        | ClientCmd::Tab(_)
        | ClientCmd::TogglePaneFullscreen => ScrollbackEditorCmdAction::Ignore,
    }
}

pub fn handle_open_client_request(
    dump_style: ScrollbackDumpStyle,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<ScrollbackEditorOpenClientOutcome> {
    if state.scrollback_editor.is_some() {
        return Ok(ScrollbackEditorOpenClientOutcome::AlreadyOpen);
    }
    let previous_pane = state.layout.active_pane_id()?;
    state.input_mode = ServerInputMode::Normal;
    let opened = self::open(
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_fullscreen,
        &state.terminal_size,
        dump_style,
    )?;
    let editor_pane_id = opened.state.editor_pane_id();
    Ok(ScrollbackEditorOpenClientOutcome::Opened {
        editor: opened.state,
        editor_pane_id,
        previous_pane,
    })
}

pub fn rollback_open_client_request(
    state: &mut ClientSessionState<'_>,
    editor: ScrollbackEditorState,
) -> rootcause::Result<()> {
    self::restore(
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_fullscreen,
        editor,
    )
}

pub fn restore_before_reap_if_needed(
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<ScrollbackEditorRestoreOutcome> {
    if state.scrollback_editor.is_none() {
        return Ok(ScrollbackEditorRestoreOutcome::unchanged());
    }
    if state.runtimes.exited_panes()?.is_empty() {
        return Ok(ScrollbackEditorRestoreOutcome::unchanged());
    }
    // Reap only against the real pane tree. The editor tree is attached-client-local; restoring first avoids
    // persisting a temporary `nvim` pane or reaping a hidden original pane against the wrong layout.
    self::write_focus_lost_if_live(state.scrollback_editor.as_ref(), state.runtimes)?;
    self::restore_without_render(state)
}

pub fn restore_without_render(state: &mut ClientSessionState<'_>) -> rootcause::Result<ScrollbackEditorRestoreOutcome> {
    let Some(editor) = state.scrollback_editor.take() else {
        return Ok(ScrollbackEditorRestoreOutcome::unchanged());
    };
    let editor_pane_id = editor.editor_pane_id();
    self::restore(
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_fullscreen,
        editor,
    )?;
    Ok(ScrollbackEditorRestoreOutcome {
        editor_pane_id: Some(editor_pane_id),
    })
}

pub fn write_focus_lost_if_live(
    editor: Option<&ScrollbackEditorState>,
    runtimes: &PaneRuntimes,
) -> rootcause::Result<()> {
    let Some(editor) = editor else {
        return Ok(());
    };
    let editor_pane_id = editor.editor_pane_id();
    if !runtimes.pane_ids().contains(&editor_pane_id) {
        return Ok(());
    }
    let handle = runtimes.handle(editor_pane_id)?;
    if !handle.has_exited() {
        // The editor is about to be restored or closed while focus is active. Notify focus-reporting apps before the
        // PTY disappears; the restored original pane receives FocusGained after restore.
        handle.write_focus_event(TerminalFocusEvent::Lost)?;
    }
    Ok(())
}

impl SessionLayout {
    fn replace_active_pane_with_scrollback_editor(&mut self, metadata: SessionMetadata) -> rootcause::Result<PaneId> {
        let editor_pane_id = PaneId::new(self.next_pane_number()?)?;
        let tab = self.active_tab_mut()?;
        let active_pane = tab.active_pane;
        let focus_seq = tab
            .pane_tree
            .pane_mut(active_pane)
            .ok_or_else(|| {
                report!("muxr active pane is missing from server layout").attach(format!("pane_id={active_pane}"))
            })?
            .focus_seq;
        let editor_pane = Pane {
            attention_state: PaneAttentionState::Idle,
            cmd_label: metadata.cmd_label.clone(),
            cwd: metadata.cwd,
            focus_seq,
            id: editor_pane_id,
            started_at: metadata.started_at,
            state: PaneState::Running,
            title: metadata.cmd_label,
        };

        if !tab.pane_tree.replace_pane(active_pane, editor_pane)? {
            return Err(
                report!("muxr active pane is missing from server layout").attach(format!("pane_id={active_pane}"))
            );
        }
        tab.active_pane = editor_pane_id;
        Ok(editor_pane_id)
    }
}

impl PaneTree {
    fn replace_pane(&mut self, pane_id: PaneId, new_pane: Pane) -> rootcause::Result<bool> {
        match self {
            Self::Pane(pane) if pane.id == pane_id => {
                *pane = new_pane;
                Ok(true)
            }
            Self::Pane(_) => Ok(false),
            Self::Split { first, second, .. } => {
                if first.replace_pane(pane_id, new_pane.clone())? {
                    return Ok(true);
                }
                second.replace_pane(pane_id, new_pane)
            }
        }
    }
}

fn create_focused_pane_dump(
    config: &ServerConfig,
    layout: &SessionLayout,
    runtimes: &PaneRuntimes,
    dump_style: ScrollbackDumpStyle,
) -> rootcause::Result<PathBuf> {
    let pane_id = layout.active_pane_id()?;
    self::write_scrollback_dump_file(&config.paths.root, pane_id, dump_style, |file| {
        runtimes.write_scrollback_dump(pane_id, dump_style, file)
    })
}

fn open(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &mut PaneRuntimes,
    pane_fullscreen: &mut PaneFullscreen,
    terminal_size: &TerminalSize,
    dump_style: ScrollbackDumpStyle,
) -> rootcause::Result<OpenScrollbackEditor> {
    let _synced = runtimes.sync_layout_terminal_titles(layout)?;
    let original_layout = layout.clone();
    let original_fullscreen = pane_fullscreen.clone();
    let original_pane_id = layout.active_pane_id()?;
    let original_pane = layout
        .pane(original_pane_id)
        .ok_or_else(|| {
            report!("muxr active pane is missing from server layout").attach(format!("pane_id={original_pane_id}"))
        })?
        .clone();
    let editor_size = self::active_pane_editor_size(layout, pane_fullscreen, terminal_size)?;
    let dump_path = self::create_focused_pane_dump(config, layout, runtimes, dump_style)?;
    let editor_cmd = self::editor_cmd(config.user_config.scrollback.editor, &dump_path)?;
    let metadata = SessionMetadata {
        cmd_label: SCROLLBACK_EDITOR_TITLE.to_owned(),
        cwd: original_pane.cwd.clone(),
        started_at: crate::server::unix_timestamp_millis()?,
    };
    let editor_pane_id = match layout.replace_active_pane_with_scrollback_editor(metadata) {
        Ok(editor_pane_id) => editor_pane_id,
        Err(error) => {
            return Err(self::remove_scrollback_dump_file_after_error(&dump_path, error));
        }
    };
    pane_fullscreen.replace_active_tab_pane(layout, original_pane_id, editor_pane_id);

    if let Err(error) = runtimes.spawn_cmd_pane(
        editor_pane_id,
        &original_pane.cwd,
        &editor_cmd,
        Some(SCROLLBACK_EDITOR_TITLE.to_owned()),
        config,
        &editor_size,
    ) {
        *layout = original_layout;
        *pane_fullscreen = original_fullscreen;
        return Err(self::remove_scrollback_dump_file_after_error(
            &dump_path,
            error.attach("failed to spawn muxr scrollback editor"),
        ));
    }

    Ok(OpenScrollbackEditor {
        state: ScrollbackEditorState {
            dump_path,
            editor_pane_id,
            original_fullscreen,
            original_layout,
        },
    })
}

fn restore(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &mut PaneRuntimes,
    pane_fullscreen: &mut PaneFullscreen,
    editor: ScrollbackEditorState,
) -> rootcause::Result<()> {
    let editor_pane_id = editor.editor_pane_id;
    runtimes.remove(editor_pane_id);
    self::remove_scrollback_dump_file(&editor.dump_path);
    self::remove_editor_pane_history(config, editor_pane_id);

    let mut original_layout = editor.original_layout;
    // Restore the real pane tree even if title sync fails; otherwise the attached state could keep pointing at the
    // temporary editor pane.
    let sync_result = runtimes.sync_layout_terminal_titles(&mut original_layout);
    let original_tab_id = original_layout.active_tab;
    let original_tab = original_layout
        .entries
        .into_iter()
        .find(|tab| tab.id == original_tab_id)
        .ok_or_else(|| report!("muxr original scrollback editor tab is missing"))?;
    let current_active_tab = layout.active_tab;
    if let Some(tab) = layout.entries.iter_mut().find(|tab| tab.id == original_tab.id) {
        *tab = original_tab;
    } else {
        layout.entries.push(original_tab);
    }
    if layout.entries.iter().any(|tab| tab.id == current_active_tab) {
        layout.active_tab = current_active_tab;
    } else {
        layout.active_tab = original_tab_id;
    }
    *pane_fullscreen = editor.original_fullscreen;
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    let _synced = sync_result?;
    Ok(())
}

fn active_pane_editor_size(
    layout: &SessionLayout,
    pane_fullscreen: &PaneFullscreen,
    terminal_size: &TerminalSize,
) -> rootcause::Result<TerminalSize> {
    let active_pane = layout.active_pane_id()?;
    let pane_layout = pane_fullscreen.pane_layout(layout, terminal_size)?;
    let active_region = pane_layout
        .regions()
        .iter()
        .find(|region| region.id == active_pane)
        .ok_or_else(|| {
            report!("muxr active pane is missing from visible layout").attach(format!("pane_id={active_pane}"))
        })?;
    TerminalSize::new(active_region.area.size.cols, active_region.area.size.rows)
}

fn editor_cmd(config: ScrollbackEditorConfig, dump_path: &Path) -> rootcause::Result<ShellCmd> {
    let mut args = config
        .args
        .iter()
        .map(|arg| self::expanded_editor_arg(arg))
        .collect::<Vec<_>>();
    args.push(dump_path.to_string_lossy().into_owned());
    ShellCmd::with_args(config.program, args)
}

fn expanded_editor_arg(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("~/") else {
        return raw.to_owned();
    };
    std::env::var_os("HOME").map_or_else(
        || raw.to_owned(),
        |home| PathBuf::from(home).join(rest).to_string_lossy().into_owned(),
    )
}

fn write_scrollback_dump_file(
    session_root: &Path,
    pane_id: PaneId,
    dump_style: ScrollbackDumpStyle,
    write_dump: impl FnOnce(&mut fs::File) -> rootcause::Result<()>,
) -> rootcause::Result<PathBuf> {
    let dump_dir = session_root.join("scrollback");
    fs::create_dir_all(&dump_dir).context("failed to create muxr scrollback dump directory")?;

    let path = dump_dir.join(self::scrollback_dump_file_name(pane_id, dump_style)?);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .context("failed to create muxr scrollback dump file")?;
    if let Err(error) =
        write_dump(&mut file).and_then(|()| Ok(file.flush().context("failed to flush muxr scrollback dump file")?))
    {
        return Err(self::remove_scrollback_dump_file_after_error(&path, error));
    }
    Ok(path)
}

fn scrollback_dump_file_name(pane_id: PaneId, dump_style: ScrollbackDumpStyle) -> rootcause::Result<String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("failed to read system time for muxr scrollback dump")?
        .as_nanos();
    Ok(format!(
        "{}-{timestamp}-{}.{}",
        pane_id,
        std::process::id(),
        self::scrollback_dump_file_extension(dump_style),
    ))
}

const fn scrollback_dump_file_extension(dump_style: ScrollbackDumpStyle) -> &'static str {
    match dump_style {
        ScrollbackDumpStyle::PlainText => "txt",
        ScrollbackDumpStyle::Ansi => "ansi",
    }
}

fn remove_scrollback_dump_file(path: &Path) {
    self::remove_scrollback_dump_file_with_event("remove_dump", path);
}

fn remove_scrollback_dump_file_with_event(event: &str, path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        // Temporary files may already be gone after editor/process cleanup; only other errors leave stale state.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => crate::session::tracing::scrollback::cleanup_failed(event, None, path, &error),
    }
}

fn remove_editor_pane_history(config: &ServerConfig, editor_pane_id: PaneId) {
    let path = self::pane_output_path(&config.paths.panes, editor_pane_id);
    if let Some(parent) = path.parent() {
        match fs::remove_dir_all(parent) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => crate::session::tracing::scrollback::cleanup_failed(
                "remove_editor_history",
                Some(editor_pane_id),
                parent,
                &error,
            ),
        }
    }
}

fn remove_scrollback_dump_file_after_error(path: &Path, error: impl Into<rootcause::Report>) -> rootcause::Report {
    self::remove_scrollback_dump_file_with_event("remove_dump_after_error", path);
    error.into()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use muxr_config::MuxrConfig;
    use muxr_core::RenderCell;

    use super::*;
    use crate::keyboard_input::TabCmd;
    use crate::pane::focus::PaneFocusDirection;
    use crate::pane::split::PaneSplitAxis;
    use crate::server::test_helpers as server_test_helpers;
    use crate::session::start_seed::SessionStartSeed;
    use crate::state::test_helpers as state_test_helpers;

    #[rstest::rstest]
    #[case::inactive_runs(
        ClientCmd::SplitPane(PaneSplitAxis::Vertical),
        false,
        ScrollbackEditorCmdAction::Run(ClientCmd::SplitPane(PaneSplitAxis::Vertical))
    )]
    #[case::active_close_restores(ClientCmd::ClosePane, true, ScrollbackEditorCmdAction::Restore)]
    #[case::active_split_is_ignored(
        ClientCmd::SplitPane(PaneSplitAxis::Vertical),
        true,
        ScrollbackEditorCmdAction::Ignore
    )]
    #[case::active_create_tab_is_ignored(ClientCmd::Tab(TabCmd::Create), true, ScrollbackEditorCmdAction::Ignore)]
    #[case::active_open_scrollback_editor_is_ignored(
        ClientCmd::OpenScrollbackEditor,
        true,
        ScrollbackEditorCmdAction::Ignore
    )]
    #[case::active_focus_pane_is_ignored(
        ClientCmd::FocusPane(PaneFocusDirection::Right),
        true,
        ScrollbackEditorCmdAction::Ignore
    )]
    fn test_scrollback_editor_cmd_action_when_editor_mode_is_active_blocks_layout_mutations(
        #[case] cmd: ClientCmd,
        #[case] editor_active: bool,
        #[case] expected: ScrollbackEditorCmdAction,
    ) {
        pretty_assertions::assert_eq!(self::cmd_action(cmd, editor_active), expected);
    }

    #[test]
    fn test_write_scrollback_editor_focus_lost_if_live_when_editor_is_reporting_writes_lost() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        let mut user_config = MuxrConfig::default();
        user_config.scrollback.editor = muxr_config::ScrollbackEditorConfig {
            program: "/bin/sh",
            args: &[
                "-c",
                "printf '\\033[?1004hready\\n'; \
                 stty raw -echo; \
                 dd bs=3 count=1 2>/dev/null | od -An -tx1 -v; \
                 sleep 30",
                "muxr-test-scrollback-editor",
            ],
        };
        config.user_config = Arc::new(user_config);
        config.shell_cmd = server_test_helpers::shell_cmd_with_args("/bin/sh", &["-c", "sleep 30"]);
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = SessionLayout::initial(&config.session, state_test_helpers::metadata("sh", 1))?;
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        let mut pane_fullscreen = PaneFullscreen::default();
        let opened = self::open(
            &config,
            &mut layout,
            &mut runtimes,
            &mut pane_fullscreen,
            &terminal_size,
            config.user_config.scrollback.dump_style,
        )?;
        let editor_pane_id = opened.state.editor_pane_id();
        self::wait_for_runtime_snapshot_contains(&runtimes, editor_pane_id, "ready")?;

        self::write_focus_lost_if_live(Some(&opened.state), &runtimes)?;

        self::wait_for_runtime_snapshot_contains(&runtimes, editor_pane_id, "1b 5b 4f")?;
        Ok(())
    }

    #[test]
    fn test_write_scrollback_dump_file_when_bytes_are_exported_creates_dump_file() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let pane_id = PaneId::new(7)?;

        let path = self::write_scrollback_dump_file(tempdir.path(), pane_id, ScrollbackDumpStyle::PlainText, |file| {
            Ok(file
                .write_all(b"one\ntwo\n")
                .context("failed to write test scrollback dump")?)
        })?;

        pretty_assertions::assert_eq!(fs::read(&path)?, b"one\ntwo\n".to_vec());
        pretty_assertions::assert_eq!(path.extension().and_then(|extension| extension.to_str()), Some("txt"));
        Ok(())
    }

    #[test]
    fn test_scrollback_cleanup_when_paths_are_missing_is_silent() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = server_test_helpers::server_config(tempdir.path(), "work")?;
        let pane_id = PaneId::new(99)?;

        let log = crate::session::tracing::collect_test_log(&config.session, || {
            self::remove_scrollback_dump_file(&tempdir.path().join("missing.txt"));
            self::remove_editor_pane_history(&config, pane_id);
            Ok(())
        })?;

        assert2::assert!(!log.contains("kind=\"scrollback_cleanup_failed\""));
        Ok(())
    }

    #[rstest::rstest]
    #[case::configured_args(
        ScrollbackEditorConfig {
            program: "nvim",
            args: &["-u", "minimal.lua", "-R", "-n", "+", "-c", "nnoremap <buffer> <silent> <Esc> :quit!<CR>"],
        },
        "nvim -u minimal.lua -R -n + -c nnoremap <buffer> <silent> <Esc> :quit!<CR> muxr-scrollback.txt"
    )]
    #[case::no_configured_args(
        ScrollbackEditorConfig {
            program: "nvim",
            args: &[],
        },
        "nvim muxr-scrollback.txt"
    )]
    fn test_editor_cmd_opens_bottom_and_maps_escape_to_quit(
        #[case] config: ScrollbackEditorConfig,
        #[case] expected: &str,
    ) -> rootcause::Result<()> {
        let cmd = self::editor_cmd(config, Path::new("muxr-scrollback.txt"))?;

        pretty_assertions::assert_eq!(cmd.label_with_args(), expected);
        Ok(())
    }

    #[test]
    fn test_replace_active_pane_with_scrollback_editor_preserves_split_shape() -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;
        let original_pane_id = layout.split_active_pane(
            muxr_config::MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;
        let original_regions =
            state_test_helpers::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?;

        let editor_pane_id = layout
            .replace_active_pane_with_scrollback_editor(state_test_helpers::metadata(SCROLLBACK_EDITOR_TITLE, 3))?;

        pretty_assertions::assert_eq!(original_pane_id.to_string(), "pane-2");
        pretty_assertions::assert_eq!(editor_pane_id.to_string(), "pane-3");
        pretty_assertions::assert_eq!(layout.active_pane_id()?, editor_pane_id);
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_ids(&layout)?,
            vec!["pane-1", "pane-3"]
        );
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                original_regions[0].clone(),
                (
                    "pane-3".to_owned(),
                    original_regions[1].1,
                    original_regions[1].2,
                    original_regions[1].3,
                    original_regions[1].4,
                ),
            ],
        );
        Ok(())
    }

    #[test]
    fn test_restore_when_editor_is_active_restores_original_layout_and_removes_editor_runtime() -> rootcause::Result<()>
    {
        let tempdir = tempfile::tempdir()?;
        let config = server_test_helpers::server_config(tempdir.path(), "work")?;
        let size = TerminalSize::new(80, 24)?;
        let mut layout = state_test_helpers::layout("work")?;
        layout.split_active_pane(
            muxr_config::MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;
        let original_layout = layout.clone();
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: original_layout.clone(),
                startup_cmds: Vec::new(),
            },
            &size,
            std::sync::Arc::new(tokio::sync::Notify::new()),
        )?;
        let dump_path = self::write_scrollback_dump_file(
            &config.paths.root,
            original_layout.active_pane_id()?,
            ScrollbackDumpStyle::PlainText,
            |file| {
                Ok(file
                    .write_all(b"history")
                    .context("failed to write test scrollback dump")?)
            },
        )?;
        let editor_pane_id = layout
            .replace_active_pane_with_scrollback_editor(state_test_helpers::metadata(SCROLLBACK_EDITOR_TITLE, 3))?;
        let editor_cwd = tempdir.path().to_string_lossy().into_owned();
        runtimes.spawn_cmd_pane(
            editor_pane_id,
            &editor_cwd,
            &server_test_helpers::shell_cmd_with_args("/bin/sh", &["-c", "sleep 30"]),
            Some(SCROLLBACK_EDITOR_TITLE.to_owned()),
            &config,
            &size,
        )?;
        let state = ScrollbackEditorState {
            dump_path: dump_path.clone(),
            editor_pane_id,
            original_fullscreen: PaneFullscreen::default(),
            original_layout: original_layout.clone(),
        };
        let layout_dir = config
            .paths
            .layout
            .parent()
            .ok_or_else(|| report!("expected test layout path parent"))?;
        fs::create_dir_all(layout_dir).context("failed to create test layout directory")?;
        let mut fullscreen = PaneFullscreen::default();

        self::restore(&config, &mut layout, &mut runtimes, &mut fullscreen, state)?;

        pretty_assertions::assert_eq!(layout.active_pane_id()?, original_layout.active_pane_id()?);
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_ids(&layout)?,
            state_test_helpers::layout_active_tab_pane_ids(&original_layout)?,
        );
        assert2::assert!(!runtimes.pane_ids().contains(&editor_pane_id));
        assert2::assert!(!dump_path.exists());
        Ok(())
    }

    fn wait_for_runtime_snapshot_contains(
        runtimes: &PaneRuntimes,
        pane_id: PaneId,
        needle: &str,
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            let snapshot = runtimes.handle(pane_id)?.render_snapshot()?;
            let rendered = snapshot
                .rows()
                .iter()
                .flat_map(|row| row.cells().iter().map(RenderCell::text))
                .collect::<String>();
            if self::snapshot_contains(&rendered, needle) {
                return Ok(());
            }
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(report!("timed out waiting for muxr runtime snapshot").attach(rendered));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn snapshot_contains(rendered: &str, needle: &str) -> bool {
        if rendered.contains(needle) {
            return true;
        }
        let needle_tokens = needle.split_whitespace().collect::<Vec<_>>();
        let rendered_tokens = rendered.split_whitespace().collect::<Vec<_>>();
        rendered_tokens
            .windows(needle_tokens.len())
            .any(|window| window == needle_tokens.as_slice())
    }
}
