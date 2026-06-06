use std::fs;
use std::path::Path;

use muxr_config::ExternalLayoutPane;
use muxr_config::ExternalSessionLayout;
use muxr_core::PaneId;
use muxr_core::SessionName;
use muxr_core::TabId;
use rootcause::report;

use crate::pty::ShellCmd;
use crate::server::ServerConfig;
use crate::state::Pane;
use crate::state::PaneAttentionState;
use crate::state::PaneState;
use crate::state::PaneTree;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;
use crate::state::Tab;

pub struct SessionStartSeed {
    pub layout: SessionLayout,
    pub startup_cmds: Vec<(PaneId, ShellCmd)>,
}

impl SessionStartSeed {
    fn from_external_layout(
        session: &SessionName,
        external_layout: &ExternalSessionLayout,
        default_shell_cmd: &ShellCmd,
        started_at: u64,
    ) -> rootcause::Result<Self> {
        external_layout.validate()?;

        let mut tabs = Vec::new();
        let mut startup_cmds = Vec::new();
        for (tab_index, external_tab) in external_layout.tabs().iter().enumerate() {
            let tab_id = TabId::new(self::layout_id(tab_index, "tab")?)?;
            let pane_id = PaneId::new(self::layout_id(tab_index, "pane")?)?;
            let external_pane = external_tab.panes().first().ok_or_else(|| {
                report!("muxr external layout tab has no panes").attach(format!("tab_index={tab_index}"))
            })?;
            if external_tab.panes().len() > 1 {
                return Err(report!("muxr external layout tab has unsupported multiple panes")
                    .attach(format!("tab_index={tab_index}")));
            }

            let pane_cmd = self::pane_startup_cmd(external_pane, default_shell_cmd)?;
            if external_pane.cmd().is_some() {
                startup_cmds.push((pane_id, pane_cmd.clone()));
            }
            let cmd_label = pane_cmd.label_with_args();
            tabs.push(Tab {
                active_pane: pane_id,
                id: tab_id,
                pane_tree: PaneTree::Pane(Pane {
                    attention_state: PaneAttentionState::Idle,
                    cmd_label: cmd_label.clone(),
                    cwd: external_tab.cwd().trim().to_owned(),
                    focus_seq: 1,
                    id: pane_id,
                    started_at,
                    state: PaneState::Running,
                    title: cmd_label,
                }),
                title: format!("tab {}", tab_id.get()),
            });
        }

        let active_tab = tabs
            .first()
            .map(|tab| tab.id)
            .ok_or_else(|| report!("muxr external layout has no tabs"))?;
        let layout = SessionLayout {
            active_tab,
            entries: tabs,
            session: session.clone(),
        };
        layout.snapshot()?;
        Ok(Self { layout, startup_cmds })
    }
}

pub fn load_session_start_seed(
    config: &ServerConfig,
    metadata: SessionMetadata,
) -> rootcause::Result<SessionStartSeed> {
    let persisted_layout = crate::state::persisted::load_metadata(&config.paths, &config.session)?;
    match (persisted_layout, config.external_layout.as_deref()) {
        (Some(_), Some(layout_path)) => Err(report!("muxr external layout can only seed a new session")
            .attach(format!("layout={}", layout_path.display()))
            .attach(format!("session={}", config.session))),
        (Some(layout), None) => Ok(SessionStartSeed {
            layout,
            startup_cmds: Vec::new(),
        }),
        (None, Some(layout_path)) => {
            let external_layout = self::load_external_layout(layout_path)?;
            SessionStartSeed::from_external_layout(
                &config.session,
                &external_layout,
                &config.shell_cmd,
                metadata.started_at,
            )
        }
        (None, None) => Ok(SessionStartSeed {
            layout: SessionLayout::initial(&config.session, metadata)?,
            startup_cmds: Vec::new(),
        }),
    }
}

fn load_external_layout(path: &Path) -> rootcause::Result<ExternalSessionLayout> {
    let bytes = fs::read(path).map_err(|error| {
        report!("failed to read muxr external layout")
            .attach(format!("path={}", path.display()))
            .attach(format!("error={error}"))
    })?;
    let layout: ExternalSessionLayout = serde_json::from_slice(&bytes).map_err(|error| {
        report!("failed to parse muxr external layout")
            .attach(format!("path={}", path.display()))
            .attach(format!("error={error}"))
    })?;
    layout.validate()?;
    Ok(layout)
}

fn pane_startup_cmd(external_pane: &ExternalLayoutPane, default_shell_cmd: &ShellCmd) -> rootcause::Result<ShellCmd> {
    let Some(cmd) = external_pane.cmd() else {
        return Ok(default_shell_cmd.clone());
    };
    ShellCmd::with_args(cmd.trim(), external_pane.args().iter().cloned())
}

fn layout_id(index: usize, kind: &str) -> rootcause::Result<u32> {
    let number = index
        .checked_add(1)
        .ok_or_else(|| report!("muxr external layout {kind} index overflowed"))?;
    u32::try_from(number).map_err(|_| report!("muxr external layout {kind} index overflowed"))
}

#[cfg(test)]
mod tests {
    use muxr_core::TerminalSize;

    use super::*;
    use crate::server::test_helpers as server_test_helpers;

    #[test]
    fn test_load_session_start_seed_when_external_layout_is_requested_builds_seed_layout() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        let layout_path = tempdir.path().join("work.json");
        std::fs::write(
            &layout_path,
            r#"{
                "tabs": [
                    {"cwd":"/tmp/one","panes":[{}]},
                    {"cwd":"/tmp/two","panes":[{"cmd":"/bin/echo","args":["seeded"]}]}
                ]
            }"#,
        )?;
        config.external_layout = Some(layout_path);

        let seed = load_session_start_seed(
            &config,
            SessionMetadata {
                cmd_label: "sh".to_owned(),
                cwd: "/ignored".to_owned(),
                started_at: 7,
            },
        )?;

        let snapshot = seed.layout.snapshot()?;
        pretty_assertions::assert_eq!(snapshot.active_tab().to_string(), "tab-1");
        pretty_assertions::assert_eq!(snapshot.tabs().len(), 2);
        pretty_assertions::assert_eq!(snapshot.tabs()[0].panes()[0].cwd, "/tmp/one");
        pretty_assertions::assert_eq!(snapshot.tabs()[0].panes()[0].cmd_label, None);
        pretty_assertions::assert_eq!(snapshot.tabs()[1].panes()[0].cwd, "/tmp/two");
        pretty_assertions::assert_eq!(snapshot.tabs()[1].panes()[0].cmd_label, None);
        pretty_assertions::assert_eq!(
            seed.layout
                .pane(PaneId::new(2)?)
                .ok_or_else(|| report!("expected seeded pane"))?
                .cmd_label,
            "echo seeded"
        );
        pretty_assertions::assert_eq!(
            seed.startup_cmds,
            vec![(PaneId::new(2)?, ShellCmd::with_args("/bin/echo", ["seeded"])?),]
        );
        pretty_assertions::assert_eq!(
            seed.layout
                .active_tab()?
                .pane_layout(&TerminalSize::new(80, 24)?)?
                .regions()
                .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn test_load_session_start_seed_when_persisted_layout_exists_and_external_layout_is_requested_returns_error()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        std::fs::create_dir_all(&config.paths.root)?;
        let initial = SessionLayout::initial(
            &config.session,
            SessionMetadata {
                cmd_label: "sh".to_owned(),
                cwd: "/tmp".to_owned(),
                started_at: 1,
            },
        )?;
        crate::state::persisted::write_metadata(&config.paths, &initial)?;
        config.external_layout = Some(tempdir.path().join("work.json"));

        assert2::assert!(
            let Err(_) = load_session_start_seed(
                &config,
                SessionMetadata {
                    cmd_label: "sh".to_owned(),
                    cwd: "/tmp".to_owned(),
                    started_at: 2,
                },
            )
        );
        Ok(())
    }

    #[test]
    fn test_load_session_start_seed_when_external_layout_is_missing_returns_error() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        config.external_layout = Some(tempdir.path().join("missing.json"));

        assert2::assert!(
            let Err(_) = load_session_start_seed(
                &config,
                SessionMetadata {
                    cmd_label: "sh".to_owned(),
                    cwd: "/tmp".to_owned(),
                    started_at: 1,
                },
            )
        );
        Ok(())
    }
}
