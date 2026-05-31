use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use muxr_core::ClientCommand;
use muxr_core::ClientKey;
use muxr_core::ClientKeyCode;
use muxr_core::ClientKeyModifiers;
use muxr_core::ClientMousePosition;
use muxr_core::ClientRequest;
use muxr_core::LayoutSnapshot;
use muxr_core::PROTOCOL_VERSION;
use muxr_core::PaneFocusDirection;
use muxr_core::PaneId;
use muxr_core::PaneResizeDirection;
use muxr_core::PaneScrollDirection;
use muxr_core::PaneSnapshot;
use muxr_core::RenderBaseline;
use muxr_core::RenderCell;
use muxr_core::RenderColor;
use muxr_core::RenderCursor;
use muxr_core::RenderDiff;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use muxr_core::RenderUpdate;
use muxr_core::ServerError;
use muxr_core::ServerEvent;
use muxr_core::ServerHello;
use muxr_core::ServerPid;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TabId;
use muxr_core::TabSnapshot;
use muxr_core::TerminalSize;
use muxr_transport::ServerConnection;
use muxr_transport::ServerEventWriter;
use muxr_transport::ServerListener;
use muxr_transport::ServerRequestReader;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use crate::history::pane_output_path;
use crate::pty::PtyEvent;
use crate::pty::PtyExitStatus;
use crate::pty::PtyHandle;
use crate::pty::PtySession;
use crate::pty::PtySinkGuard;
pub use crate::pty::ShellCommand;
use crate::terminal::TerminalSnapshot;

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CLIENT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(test)]
const CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(test)]
const CLIENT_HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(500);
#[cfg(not(test))]
const CLIENT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
const CLIENT_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(test)]
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
#[cfg(not(test))]
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const GROUP_OR_OTHER_PERMISSIONS_MASK: u32 = 0o077;
const OUTPUT_EVENT_CHANNEL_LIMIT: usize = 1024;
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_SOCKET_MODE: u32 = 0o600;
const RENDER_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const INITIAL_PANE_ID: &str = "pane-1";
const INITIAL_TAB_ID: &str = "tab-1";
const INITIAL_TAB_TITLE: &str = "default";
const LAYOUT_VERSION: u16 = 4;
const SPLIT_RATIO_SCALE: u16 = 1000;
const SPLIT_RATIO_HALF_SCALE: u16 = 500;
const DEFAULT_SPLIT_RATIO: u16 = 500;
const MIN_SPLIT_RATIO: u16 = 50;
const MAX_SPLIT_RATIO: u16 = 950;
const SPLIT_RESIZE_STEP: u16 = 50;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub session: SessionName,
    pub paths: SessionPaths,
    pub max_accepted_connections: Option<usize>,
    pub shell_command: ShellCommand,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionMetadata {
    command_label: String,
    cwd: String,
    started_at: u64,
}

impl SessionMetadata {
    fn new(config: &ServerConfig) -> rootcause::Result<Self> {
        Ok(Self {
            command_label: config.shell_command.label(),
            cwd: std::env::current_dir()
                .context("failed to read muxr server cwd")?
                .to_string_lossy()
                .into_owned(),
            started_at: self::unix_timestamp_millis()?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct SessionLayout {
    active_tab: TabId,
    session: SessionName,
    tabs: Vec<SessionTab>,
}

impl SessionLayout {
    fn initial(config: &ServerConfig, metadata: SessionMetadata) -> rootcause::Result<Self> {
        let pane_id = PaneId::new(INITIAL_PANE_ID)?;
        let tab_id = TabId::new(INITIAL_TAB_ID)?;

        Ok(Self {
            active_tab: tab_id.clone(),
            session: config.session.clone(),
            tabs: vec![SessionTab {
                active_pane: pane_id.clone(),
                id: tab_id,
                pane_tree: PaneNode::leaf(SessionPane {
                    command_label: metadata.command_label.clone(),
                    cwd: metadata.cwd,
                    exit_status: None,
                    exited_at: None,
                    focus_seq: 1,
                    id: pane_id,
                    started_at: metadata.started_at,
                    title: metadata.command_label,
                }),
                title: INITIAL_TAB_TITLE.to_owned(),
            }],
        })
    }

    fn from_persisted(config: &ServerConfig, persisted: PersistedLayoutOwned) -> rootcause::Result<Self> {
        if persisted.version != LAYOUT_VERSION {
            return Err(report!("unsupported muxr layout metadata version")
                .attach(format!("expected={LAYOUT_VERSION}"))
                .attach(format!("actual={}", persisted.version)));
        }
        if persisted.session != config.session {
            return Err(report!("muxr layout metadata session mismatch")
                .attach(format!("expected={}", config.session))
                .attach(format!("actual={}", persisted.session)));
        }

        let layout = Self {
            active_tab: persisted.active_tab,
            session: persisted.session,
            tabs: persisted.tabs,
        };
        drop(layout.snapshot()?);
        Ok(layout)
    }

    fn snapshot(&self) -> rootcause::Result<LayoutSnapshot> {
        let tabs = self
            .tabs
            .iter()
            .map(SessionTab::snapshot)
            .collect::<rootcause::Result<Vec<_>>>()?;
        LayoutSnapshot::new(self.active_tab.clone(), tabs)
    }

    fn create_tab(&mut self, metadata: SessionMetadata) -> rootcause::Result<PaneId> {
        let tab_index = self.active_tab_index()?;
        let tab_number = self.next_tab_number()?;
        let pane_number = self.next_pane_number()?;
        let tab_id = TabId::new(format!("tab-{tab_number}"))?;
        let pane_id = PaneId::new(format!("pane-{pane_number}"))?;
        let insert_index = tab_index
            .checked_add(1)
            .ok_or_else(|| report!("muxr tab insert index overflowed"))?;

        self.tabs.insert(
            insert_index,
            SessionTab {
                active_pane: pane_id.clone(),
                id: tab_id.clone(),
                pane_tree: PaneNode::leaf(SessionPane {
                    command_label: metadata.command_label.clone(),
                    cwd: metadata.cwd,
                    exit_status: None,
                    exited_at: None,
                    focus_seq: 1,
                    id: pane_id.clone(),
                    started_at: metadata.started_at,
                    title: metadata.command_label,
                }),
                title: format!("tab {tab_number}"),
            },
        );
        self.active_tab = tab_id;
        Ok(pane_id)
    }

    fn split_active_pane(&mut self, metadata: SessionMetadata, split_axis: PaneSplitAxis) -> rootcause::Result<PaneId> {
        let pane_number = self.next_pane_number()?;
        let pane_id = PaneId::new(format!("pane-{pane_number}"))?;
        let tab = self.active_tab_mut()?;
        let focus_seq = tab.next_focus_seq()?;
        let new_pane = SessionPane {
            command_label: metadata.command_label.clone(),
            cwd: metadata.cwd,
            exit_status: None,
            exited_at: None,
            focus_seq,
            id: pane_id.clone(),
            started_at: metadata.started_at,
            title: metadata.command_label,
        };
        tab.split_active_pane(&new_pane, split_axis)?;
        tab.active_pane = pane_id.clone();
        Ok(pane_id)
    }

    fn resize_active_pane(&mut self, direction: PaneResizeDirection) -> rootcause::Result<bool> {
        self.active_tab_mut()?.resize_active_pane(direction)
    }

    fn focus_pane_at(&mut self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<bool> {
        self.active_tab_mut()?.focus_pane_at(size, position)
    }

    fn focus_pane_direction(&mut self, size: &TerminalSize, direction: PaneFocusDirection) -> rootcause::Result<bool> {
        self.active_tab_mut()?.focus_pane_direction(size, direction)
    }

    fn pane_at(&self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<Option<PaneId>> {
        self.active_tab()?.pane_at(size, position)
    }

    fn close_active_pane(&mut self, exited_at: u64) -> rootcause::Result<ClosePaneOutcome> {
        let active_tab_index = self.active_tab_index()?;
        let final_pane = self.tabs.len() == 1
            && self
                .tabs
                .get(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
                .pane_count()
                == 1;
        let active_pane = self
            .tabs
            .get(active_tab_index)
            .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
            .active_pane
            .clone();

        if final_pane {
            self.tabs
                .get_mut(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
                .mark_pane_exited(&active_pane, exited_at, None)?;
            return Ok(ClosePaneOutcome {
                final_pane: true,
                pane_id: active_pane,
            });
        }

        if self
            .tabs
            .get(active_tab_index)
            .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
            .pane_count()
            == 1
        {
            self.remove_tab_at(active_tab_index)?;
        } else {
            let tab = self
                .tabs
                .get_mut(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?;
            let fallback_pane = tab.remove_pane(&active_pane)?;
            let _focused = tab.focus_pane(fallback_pane)?;
        }

        Ok(ClosePaneOutcome {
            final_pane: false,
            pane_id: active_pane,
        })
    }

    fn remove_exited_pane(
        &mut self,
        pane_id: &PaneId,
        exited_at: u64,
        exit_status: Option<PtyExitStatus>,
    ) -> rootcause::Result<PaneExitOutcome> {
        let tab_index = self.pane_tab_index(pane_id)?;

        if self.tabs.len() == 1
            && self
                .tabs
                .get(tab_index)
                .ok_or_else(|| report!("muxr exited pane tab is missing"))?
                .pane_count()
                == 1
        {
            let tab = self
                .tabs
                .get_mut(tab_index)
                .ok_or_else(|| report!("muxr final pane tab is missing"))?;
            tab.mark_pane_exited(pane_id, exited_at, exit_status)?;
            return Ok(PaneExitOutcome::Final);
        }

        if self
            .tabs
            .get(tab_index)
            .ok_or_else(|| report!("muxr exited pane tab is missing"))?
            .pane_count()
            == 1
        {
            self.remove_tab_at(tab_index)?;
            return Ok(PaneExitOutcome::Removed);
        }

        let tab = self
            .tabs
            .get_mut(tab_index)
            .ok_or_else(|| report!("muxr exited pane tab is missing"))?;
        let removed_active_pane = tab.active_pane == *pane_id;
        let fallback_pane = tab.remove_pane(pane_id)?;
        if removed_active_pane {
            let _focused = tab.focus_pane(fallback_pane)?;
        }
        Ok(PaneExitOutcome::Removed)
    }

    fn focus_previous_tab(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let previous_index = if tab_index == 0 {
            self.tabs.len().saturating_sub(1)
        } else {
            tab_index.saturating_sub(1)
        };
        self.active_tab = self
            .tabs
            .get(previous_index)
            .ok_or_else(|| report!("muxr previous tab is missing from server layout"))?
            .id
            .clone();
        Ok(())
    }

    fn focus_next_tab(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let next_index = tab_index
            .checked_add(1)
            .filter(|index| *index < self.tabs.len())
            .unwrap_or(0);
        self.active_tab = self
            .tabs
            .get(next_index)
            .ok_or_else(|| report!("muxr next tab is missing from server layout"))?
            .id
            .clone();
        Ok(())
    }

    fn move_active_tab_previous(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        if tab_index > 0 {
            self.tabs.swap(tab_index, tab_index.saturating_sub(1));
        }
        Ok(())
    }

    fn move_active_tab_next(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let Some(next_index) = tab_index.checked_add(1) else {
            return Err(report!("muxr next tab index overflowed"));
        };
        if next_index < self.tabs.len() {
            self.tabs.swap(tab_index, next_index);
        }
        Ok(())
    }

    fn active_tab_index(&self) -> rootcause::Result<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .ok_or_else(|| {
                report!("muxr active tab is missing from server layout")
                    .attach(format!("active_tab={}", self.active_tab))
            })
    }

    fn active_tab(&self) -> rootcause::Result<&SessionTab> {
        self.tabs.iter().find(|tab| tab.id == self.active_tab).ok_or_else(|| {
            report!("muxr active tab is missing from server layout").attach(format!("active_tab={}", self.active_tab))
        })
    }

    fn active_tab_mut(&mut self) -> rootcause::Result<&mut SessionTab> {
        let active_tab = self.active_tab.clone();
        self.tabs.iter_mut().find(|tab| tab.id == active_tab).ok_or_else(|| {
            report!("muxr active tab is missing from server layout").attach(format!("active_tab={active_tab}"))
        })
    }

    fn active_pane_id(&self) -> rootcause::Result<PaneId> {
        Ok(self.active_tab()?.active_pane.clone())
    }

    fn pane_regions(&self, size: &TerminalSize) -> rootcause::Result<Vec<PaneRegion>> {
        Ok(self.pane_layout(size)?.regions)
    }

    fn pane_layout(&self, size: &TerminalSize) -> rootcause::Result<PaneLayout> {
        self.active_tab()?.pane_layout(size)
    }

    fn pane_tab_index(&self, pane_id: &PaneId) -> rootcause::Result<usize> {
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            if tab.contains_pane(pane_id) {
                return Ok(tab_index);
            }
        }

        Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")))
    }

    fn remove_tab_at(&mut self, tab_index: usize) -> rootcause::Result<()> {
        self.tabs.remove(tab_index);
        if !self.tabs.iter().any(|tab| tab.id == self.active_tab) {
            let next_tab_index = if tab_index >= self.tabs.len() {
                tab_index.saturating_sub(1)
            } else {
                tab_index
            };
            let next_tab = self
                .tabs
                .get(next_tab_index)
                .ok_or_else(|| report!("muxr next tab is missing after pane removal"))?;
            self.active_tab = next_tab.id.clone();
        }
        Ok(())
    }

    fn next_tab_number(&self) -> rootcause::Result<u64> {
        self::next_number(self.tabs.iter().map(|tab| tab.id.as_ref()), "tab-")
    }

    fn next_pane_number(&self) -> rootcause::Result<u64> {
        self::next_number(self.tabs.iter().flat_map(SessionTab::pane_ids), "pane-")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum PaneSplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct PaneSplitRatio(u16);

impl PaneSplitRatio {
    const fn balanced() -> Self {
        Self(DEFAULT_SPLIT_RATIO)
    }

    fn new(value: u16) -> rootcause::Result<Self> {
        if !(MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(&value) {
            return Err(report!("muxr pane split ratio is outside supported bounds")
                .attach(format!("min={MIN_SPLIT_RATIO}"))
                .attach(format!("max={MAX_SPLIT_RATIO}"))
                .attach(format!("actual={value}")));
        }
        Ok(Self(value))
    }

    fn resized(self, resize: PaneSplitResize) -> Self {
        match resize {
            PaneSplitResize::DecreaseFirst => Self(self.0.saturating_sub(SPLIT_RESIZE_STEP).max(MIN_SPLIT_RATIO)),
            PaneSplitResize::IncreaseFirst => Self(self.0.saturating_add(SPLIT_RESIZE_STEP).min(MAX_SPLIT_RATIO)),
        }
    }

    fn split_lengths(self, total: u16) -> rootcause::Result<(u16, u16)> {
        if total < 2 {
            return Err(report!("muxr terminal is too small for pane split").attach(format!("cells={total}")));
        }
        let max_first = total
            .checked_sub(1)
            .ok_or_else(|| report!("muxr pane split max length underflowed"))?;

        let scaled = u32::from(total)
            .checked_mul(u32::from(self.0))
            .ok_or_else(|| report!("muxr pane split ratio multiplication overflowed"))?;
        let rounded = scaled
            .checked_add(u32::from(SPLIT_RATIO_HALF_SCALE))
            .ok_or_else(|| report!("muxr pane split ratio rounding overflowed"))?;
        let first = rounded
            .checked_div(u32::from(SPLIT_RATIO_SCALE))
            .ok_or_else(|| report!("muxr pane split ratio divisor was zero"))?
            .clamp(1, u32::from(max_first));
        let first = u16::try_from(first).context("muxr pane split ratio result overflowed")?;
        let second = total
            .checked_sub(first)
            .ok_or_else(|| report!("muxr pane split ratio produced an invalid second length"))?;

        Ok((first, second))
    }
}

impl<'de> Deserialize<'de> for PaneSplitRatio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(|error| serde::de::Error::custom(format!("{error:#}")))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneSplitResize {
    DecreaseFirst,
    IncreaseFirst,
}

impl PaneSplitResize {
    const fn for_direction(axis: PaneSplitAxis, direction: PaneResizeDirection) -> Option<Self> {
        match (axis, direction) {
            (PaneSplitAxis::Horizontal, PaneResizeDirection::Up)
            | (PaneSplitAxis::Vertical, PaneResizeDirection::Left) => Some(Self::DecreaseFirst),
            (PaneSplitAxis::Horizontal, PaneResizeDirection::Down)
            | (PaneSplitAxis::Vertical, PaneResizeDirection::Right) => Some(Self::IncreaseFirst),
            (PaneSplitAxis::Horizontal, PaneResizeDirection::Left | PaneResizeDirection::Right)
            | (PaneSplitAxis::Vertical, PaneResizeDirection::Down | PaneResizeDirection::Up) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClosePaneOutcome {
    final_pane: bool,
    pane_id: PaneId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneExitOutcome {
    Final,
    Removed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SessionTab {
    active_pane: PaneId,
    id: TabId,
    pane_tree: PaneNode,
    title: String,
}

impl SessionTab {
    fn snapshot(&self) -> rootcause::Result<TabSnapshot> {
        let panes = self.panes().into_iter().map(SessionPane::snapshot).collect();
        TabSnapshot::new(self.id.clone(), self.title.clone(), self.active_pane.clone(), panes)
    }

    fn split_active_pane(&mut self, new_pane: &SessionPane, split_axis: PaneSplitAxis) -> rootcause::Result<()> {
        if !self.pane_tree.split_pane(&self.active_pane, new_pane, split_axis)? {
            return Err(report!("muxr active pane is missing from server layout")
                .attach(format!("active_pane={}", self.active_pane)));
        }
        Ok(())
    }

    fn resize_active_pane(&mut self, direction: PaneResizeDirection) -> rootcause::Result<bool> {
        self.pane_tree.resize_pane(&self.active_pane, direction)
    }

    fn focus_pane_at(&mut self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<bool> {
        let Some(pane_id) = self.pane_at(size, position)? else {
            return Ok(false);
        };

        self.focus_pane(pane_id)
    }

    fn focus_pane_direction(&mut self, size: &TerminalSize, direction: PaneFocusDirection) -> rootcause::Result<bool> {
        let layout = self.pane_layout(size)?;
        let active_region = layout
            .regions
            .iter()
            .find(|region| region.id == self.active_pane)
            .ok_or_else(|| {
                report!("muxr active pane is missing from active tab layout")
                    .attach(format!("active_pane={}", self.active_pane))
            })?;
        let Some(next_pane_id) = layout
            .regions
            .iter()
            .filter(|region| region.id != active_region.id)
            .filter(|region| region.is_adjacent_to(active_region, direction))
            .max_by_key(|region| region.focus_seq)
            .map(|region| region.id.clone())
        else {
            return Ok(false);
        };

        self.focus_pane(next_pane_id)
    }

    fn focus_pane(&mut self, pane_id: PaneId) -> rootcause::Result<bool> {
        if self.active_pane == pane_id {
            return Ok(false);
        }

        let focus_seq = self.next_focus_seq()?;
        let Some(pane) = self.pane_tree.pane_mut(&pane_id) else {
            return Err(report!("muxr pane is missing from active tab").attach(format!("pane_id={pane_id}")));
        };
        pane.focus_seq = focus_seq;
        self.active_pane = pane_id;
        Ok(true)
    }

    fn pane_at(&self, size: &TerminalSize, position: ClientMousePosition) -> rootcause::Result<Option<PaneId>> {
        Ok(self
            .pane_layout(size)?
            .regions
            .iter()
            .find(|region| region.contains(position.row, position.col))
            .map(|region| region.id.clone()))
    }

    fn remove_pane(&mut self, pane_id: &PaneId) -> rootcause::Result<PaneId> {
        self.pane_tree.remove_pane(pane_id)
    }

    fn mark_pane_exited(
        &mut self,
        pane_id: &PaneId,
        exited_at: u64,
        exit_status: Option<PtyExitStatus>,
    ) -> rootcause::Result<()> {
        let Some(pane) = self.pane_tree.pane_mut(pane_id) else {
            return Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")));
        };

        pane.exited_at = Some(exited_at);
        pane.exit_status = exit_status;
        Ok(())
    }

    fn pane_count(&self) -> usize {
        self.pane_tree.pane_count()
    }

    fn contains_pane(&self, pane_id: &PaneId) -> bool {
        self.pane_tree.contains_pane(pane_id)
    }

    fn pane_ids(&self) -> Vec<&str> {
        let mut ids = Vec::new();
        self.pane_tree.append_pane_ids(&mut ids);
        ids
    }

    fn panes(&self) -> Vec<&SessionPane> {
        let mut panes = Vec::new();
        self.pane_tree.append_panes(&mut panes);
        panes
    }

    fn next_focus_seq(&self) -> rootcause::Result<u64> {
        self.panes()
            .iter()
            .map(|pane| pane.focus_seq)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| report!("muxr pane focus sequence overflowed"))
    }

    fn pane_layout(&self, size: &TerminalSize) -> rootcause::Result<PaneLayout> {
        let mut layout = PaneLayout::default();
        self.pane_tree
            .append_layout(0, 0, size.rows(), size.cols(), &mut layout)?;
        Ok(layout)
    }
}

// Pane splits are a tree so a new split mutates only the active leaf; a tab-wide axis would reflow siblings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PaneNode {
    Leaf {
        pane: SessionPane,
    },
    Split {
        axis: PaneSplitAxis,
        first_ratio: PaneSplitRatio,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl PaneNode {
    const fn leaf(pane: SessionPane) -> Self {
        Self::Leaf { pane }
    }

    fn split_pane(
        &mut self,
        pane_id: &PaneId,
        new_pane: &SessionPane,
        split_axis: PaneSplitAxis,
    ) -> rootcause::Result<bool> {
        match self {
            Self::Leaf { pane } if pane.id == *pane_id => {
                let old_pane = std::mem::replace(self, Self::leaf(new_pane.clone()));
                *self = Self::Split {
                    axis: split_axis,
                    first_ratio: PaneSplitRatio::balanced(),
                    first: Box::new(old_pane),
                    second: Box::new(Self::leaf(new_pane.clone())),
                };
                Ok(true)
            }
            Self::Leaf { .. } => Ok(false),
            Self::Split { first, second, .. } => {
                if first.split_pane(pane_id, new_pane, split_axis)? {
                    return Ok(true);
                }
                second.split_pane(pane_id, new_pane, split_axis)
            }
        }
    }

    fn resize_pane(&mut self, pane_id: &PaneId, direction: PaneResizeDirection) -> rootcause::Result<bool> {
        match self {
            Self::Leaf { .. } => Ok(false),
            Self::Split {
                axis,
                first_ratio,
                first,
                second,
            } => {
                let child_resized = if first.contains_pane(pane_id) {
                    first.resize_pane(pane_id, direction)?
                } else if second.contains_pane(pane_id) {
                    second.resize_pane(pane_id, direction)?
                } else {
                    return Ok(false);
                };
                if child_resized {
                    return Ok(true);
                }

                let Some(resize) = PaneSplitResize::for_direction(*axis, direction) else {
                    return Ok(false);
                };
                let resized_ratio = first_ratio.resized(resize);
                if resized_ratio == *first_ratio {
                    return Ok(false);
                }

                *first_ratio = resized_ratio;
                Ok(true)
            }
        }
    }

    fn remove_pane(&mut self, pane_id: &PaneId) -> rootcause::Result<PaneId> {
        let Some(fallback_pane) = self.remove_leaf(pane_id)? else {
            return Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")));
        };
        Ok(fallback_pane)
    }

    fn remove_leaf(&mut self, pane_id: &PaneId) -> rootcause::Result<Option<PaneId>> {
        match self {
            Self::Leaf { pane } if pane.id == *pane_id => {
                Err(report!("muxr cannot remove a pane leaf without a sibling").attach(format!("pane_id={pane_id}")))
            }
            Self::Split { first, second, .. } if first.contains_pane(pane_id) => {
                if first.pane_count() == 1 {
                    let replacement = (**second).clone();
                    let fallback_pane = replacement.first_pane_id()?;
                    *self = replacement;
                    Ok(Some(fallback_pane))
                } else {
                    first.remove_leaf(pane_id)
                }
            }
            Self::Split { first, second, .. } if second.contains_pane(pane_id) => {
                if second.pane_count() == 1 {
                    let replacement = (**first).clone();
                    let fallback_pane = replacement.first_pane_id()?;
                    *self = replacement;
                    Ok(Some(fallback_pane))
                } else {
                    second.remove_leaf(pane_id)
                }
            }
            Self::Leaf { .. } | Self::Split { .. } => Ok(None),
        }
    }

    fn pane_count(&self) -> usize {
        match self {
            Self::Leaf { .. } => 1,
            Self::Split { first, second, .. } => first.pane_count().saturating_add(second.pane_count()),
        }
    }

    fn contains_pane(&self, pane_id: &PaneId) -> bool {
        match self {
            Self::Leaf { pane } => pane.id == *pane_id,
            Self::Split { first, second, .. } => first.contains_pane(pane_id) || second.contains_pane(pane_id),
        }
    }

    fn first_pane_id(&self) -> rootcause::Result<PaneId> {
        match self {
            Self::Leaf { pane } => Ok(pane.id.clone()),
            Self::Split { first, .. } => first.first_pane_id(),
        }
    }

    fn pane_mut(&mut self, pane_id: &PaneId) -> Option<&mut SessionPane> {
        match self {
            Self::Leaf { pane } if pane.id == *pane_id => Some(pane),
            Self::Leaf { .. } => None,
            Self::Split { first, second, .. } => first.pane_mut(pane_id).or_else(|| second.pane_mut(pane_id)),
        }
    }

    fn append_pane_ids<'a>(&'a self, ids: &mut Vec<&'a str>) {
        match self {
            Self::Leaf { pane } => ids.push(pane.id.as_ref()),
            Self::Split { first, second, .. } => {
                first.append_pane_ids(ids);
                second.append_pane_ids(ids);
            }
        }
    }

    fn append_panes<'a>(&'a self, panes: &mut Vec<&'a SessionPane>) {
        match self {
            Self::Leaf { pane } => panes.push(pane),
            Self::Split { first, second, .. } => {
                first.append_panes(panes);
                second.append_panes(panes);
            }
        }
    }

    fn append_layout(
        &self,
        row: u16,
        col: u16,
        rows: u16,
        cols: u16,
        layout: &mut PaneLayout,
    ) -> rootcause::Result<()> {
        match self {
            Self::Leaf { pane } => {
                layout.regions.push(PaneRegion {
                    col,
                    cols,
                    focus_seq: pane.focus_seq,
                    id: pane.id.clone(),
                    row,
                    rows,
                });
                Ok(())
            }
            Self::Split {
                axis,
                first_ratio,
                first,
                second,
            } => match axis {
                PaneSplitAxis::Horizontal => {
                    let content_rows = rows
                        .checked_sub(1)
                        .ok_or_else(|| report!("muxr terminal is too small for horizontal pane border"))?;
                    let (first_rows, second_rows) = first_ratio.split_lengths(content_rows)?;
                    let border_row = row
                        .checked_add(first_rows)
                        .ok_or_else(|| report!("muxr pane border row overflowed"))?;
                    let second_row = row
                        .checked_add(first_rows)
                        .and_then(|value| value.checked_add(1))
                        .ok_or_else(|| report!("muxr pane split row overflowed"))?;
                    first.append_layout(row, col, first_rows, cols, layout)?;
                    layout.borders.push(PaneBorder {
                        axis: PaneBorderAxis::Horizontal,
                        col,
                        len: cols,
                        row: border_row,
                    });
                    second.append_layout(second_row, col, second_rows, cols, layout)
                }
                PaneSplitAxis::Vertical => {
                    let content_cols = cols
                        .checked_sub(1)
                        .ok_or_else(|| report!("muxr terminal is too small for vertical pane border"))?;
                    let (first_cols, second_cols) = first_ratio.split_lengths(content_cols)?;
                    let border_col = col
                        .checked_add(first_cols)
                        .ok_or_else(|| report!("muxr pane border col overflowed"))?;
                    let second_col = col
                        .checked_add(first_cols)
                        .and_then(|value| value.checked_add(1))
                        .ok_or_else(|| report!("muxr pane split col overflowed"))?;
                    first.append_layout(row, col, rows, first_cols, layout)?;
                    layout.borders.push(PaneBorder {
                        axis: PaneBorderAxis::Vertical,
                        col: border_col,
                        len: rows,
                        row,
                    });
                    second.append_layout(row, second_col, rows, second_cols, layout)
                }
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SessionPane {
    command_label: String,
    cwd: String,
    exit_status: Option<PtyExitStatus>,
    exited_at: Option<u64>,
    focus_seq: u64,
    id: PaneId,
    started_at: u64,
    title: String,
}

impl SessionPane {
    fn snapshot(&self) -> PaneSnapshot {
        PaneSnapshot::new(self.id.clone(), self.title.clone())
    }
}

#[derive(Serialize)]
struct PersistedLayout<'a> {
    version: u16,
    session: &'a SessionName,
    active_tab: &'a TabId,
    tabs: &'a [SessionTab],
}

#[derive(Deserialize)]
struct PersistedLayoutOwned {
    version: u16,
    session: SessionName,
    active_tab: TabId,
    tabs: Vec<SessionTab>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PaneLayout {
    borders: Vec<PaneBorder>,
    regions: Vec<PaneRegion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PaneBorder {
    axis: PaneBorderAxis,
    col: u16,
    len: u16,
    row: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneBorderAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PaneRegion {
    col: u16,
    cols: u16,
    focus_seq: u64,
    id: PaneId,
    row: u16,
    rows: u16,
}

impl PaneRegion {
    const fn contains(&self, row: u16, col: u16) -> bool {
        let Some(end_row) = self.row.checked_add(self.rows) else {
            return false;
        };
        let Some(end_col) = self.col.checked_add(self.cols) else {
            return false;
        };

        row >= self.row && row < end_row && col >= self.col && col < end_col
    }

    fn is_adjacent_to(&self, other: &Self, direction: PaneFocusDirection) -> bool {
        // Muxr pane regions exclude the separator cell, so visible neighbors have a one-cell gap where Zellij's
        // frame-inclusive pane geometry uses exact edge equality.
        match direction {
            PaneFocusDirection::Left => self.is_directly_left_of(other) && self.horizontally_overlaps_with(other),
            PaneFocusDirection::Right => self.is_directly_right_of(other) && self.horizontally_overlaps_with(other),
            PaneFocusDirection::Up => self.is_directly_above(other) && self.vertically_overlaps_with(other),
            PaneFocusDirection::Down => self.is_directly_below(other) && self.vertically_overlaps_with(other),
        }
    }

    fn is_directly_left_of(&self, other: &Self) -> bool {
        Self::edges_are_adjacent(self.end_col(), u32::from(other.col))
    }

    fn is_directly_right_of(&self, other: &Self) -> bool {
        Self::edges_are_adjacent(other.end_col(), u32::from(self.col))
    }

    fn is_directly_above(&self, other: &Self) -> bool {
        Self::edges_are_adjacent(self.end_row(), u32::from(other.row))
    }

    fn is_directly_below(&self, other: &Self) -> bool {
        Self::edges_are_adjacent(other.end_row(), u32::from(self.row))
    }

    fn horizontally_overlaps_with(&self, other: &Self) -> bool {
        Self::ranges_overlap(
            u32::from(self.row),
            u32::from(self.rows),
            u32::from(other.row),
            u32::from(other.rows),
        )
    }

    fn vertically_overlaps_with(&self, other: &Self) -> bool {
        Self::ranges_overlap(
            u32::from(self.col),
            u32::from(self.cols),
            u32::from(other.col),
            u32::from(other.cols),
        )
    }

    fn end_col(&self) -> u32 {
        u32::from(self.col).saturating_add(u32::from(self.cols))
    }

    fn end_row(&self) -> u32 {
        u32::from(self.row).saturating_add(u32::from(self.rows))
    }

    fn edges_are_adjacent(edge: u32, start: u32) -> bool {
        edge == start || edge.checked_add(1) == Some(start)
    }

    const fn ranges_overlap(first_start: u32, first_len: u32, second_start: u32, second_len: u32) -> bool {
        let first_end = first_start.saturating_add(first_len);
        let second_end = second_start.saturating_add(second_len);

        first_start < second_end && second_start < first_end
    }
}

struct PaneRuntime {
    id: PaneId,
    session: PtySession,
}

struct PaneRuntimes {
    panes: Vec<PaneRuntime>,
}

impl PaneRuntimes {
    fn spawn_for_layout(config: &ServerConfig, layout: &SessionLayout, size: &TerminalSize) -> rootcause::Result<Self> {
        let mut panes = Vec::new();
        for tab in &layout.tabs {
            for pane in tab.panes() {
                panes.push(PaneRuntime {
                    id: pane.id.clone(),
                    session: PtySession::spawn(
                        &config.shell_command,
                        size,
                        &self::pane_output_path(&config.paths.panes, &pane.id),
                    )?,
                });
            }
        }
        Ok(Self { panes })
    }

    fn spawn_pane(&mut self, pane_id: PaneId, config: &ServerConfig, size: &TerminalSize) -> rootcause::Result<()> {
        let history_path = self::pane_output_path(&config.paths.panes, &pane_id);
        self.panes.push(PaneRuntime {
            id: pane_id,
            session: PtySession::spawn(&config.shell_command, size, &history_path)?,
        });
        Ok(())
    }

    fn handle(&self, pane_id: &PaneId) -> rootcause::Result<PtyHandle> {
        self.panes
            .iter()
            .find(|pane| pane.id == *pane_id)
            .map(|pane| pane.session.handle())
            .ok_or_else(|| report!("muxr pane runtime is missing").attach(format!("pane_id={pane_id}")))
    }

    fn remove(&mut self, pane_id: &PaneId) {
        self.panes.retain(|pane| pane.id != *pane_id);
    }

    const fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    fn exited_panes(&self) -> rootcause::Result<Vec<(PaneId, Option<PtyExitStatus>)>> {
        let mut exited_panes = Vec::new();
        for pane in &self.panes {
            let handle = pane.session.handle();
            if handle.has_exited()? {
                exited_panes.push((pane.id.clone(), handle.exit_status()?));
            }
        }
        Ok(exited_panes)
    }

    fn resize_panes(&self, regions: &[PaneRegion]) -> rootcause::Result<()> {
        for region in regions {
            self.handle(&region.id)?
                .resize(&TerminalSize::new(region.cols, region.rows)?)?;
        }
        Ok(())
    }

    fn snapshot(&self, pane_id: &PaneId) -> rootcause::Result<TerminalSnapshot> {
        self.handle(pane_id)?.render_snapshot()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompositeFrame {
    cursor: RenderCursor,
    rows: Vec<RenderRowSpan>,
    seq: u64,
    size: TerminalSize,
}

#[derive(Default)]
struct RenderComposer {
    last_sent: Option<CompositeFrame>,
    next_seq: u64,
}

impl RenderComposer {
    const fn new() -> Self {
        Self {
            last_sent: None,
            next_seq: 1,
        }
    }

    fn render_baseline(
        &mut self,
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
    ) -> rootcause::Result<RenderUpdate> {
        let mut frame = Self::current_frame(layout, runtimes, size)?;
        frame.seq = self.next_sequence()?;
        let baseline = RenderBaseline::new(frame.seq, frame.size.clone(), frame.cursor.clone(), frame.rows.clone())?;
        self.last_sent = Some(frame);
        Ok(RenderUpdate::Baseline(baseline))
    }

    fn render_diff(
        &mut self,
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let Some(previous) = self.last_sent.clone() else {
            return Ok(Some(self.render_baseline(layout, runtimes, size)?));
        };
        let mut frame = Self::current_frame(layout, runtimes, size)?;
        if frame.size != previous.size {
            return Ok(Some(self.render_baseline(layout, runtimes, size)?));
        }

        let rows = previous
            .rows
            .iter()
            .zip(frame.rows.iter())
            .filter(|(previous_row, current_row)| previous_row != current_row)
            .map(|(_previous_row, current_row)| current_row.clone())
            .collect::<Vec<_>>();
        if rows.is_empty() && frame.cursor == previous.cursor {
            return Ok(None);
        }

        frame.seq = self.next_sequence()?;
        let diff = RenderDiff::new(previous.seq, frame.seq, frame.size.clone(), frame.cursor.clone(), rows)?;
        self.last_sent = Some(frame);
        Ok(Some(RenderUpdate::Diff(diff)))
    }

    fn current_frame(
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
    ) -> rootcause::Result<CompositeFrame> {
        let pane_layout = layout.pane_layout(size)?;
        let active_pane = layout.active_pane_id()?;
        let mut rows = self::empty_render_rows(size);
        let mut cursor = RenderCursor::new(0, 0, false);

        for region in &pane_layout.regions {
            let snapshot = runtimes.snapshot(&region.id)?;
            self::paste_snapshot(&mut rows, region, &snapshot)?;
            if region.id == active_pane && snapshot.cursor().visible {
                let row = region
                    .row
                    .checked_add(snapshot.cursor().row)
                    .ok_or_else(|| report!("muxr composite cursor row overflowed"))?;
                let col = region
                    .col
                    .checked_add(snapshot.cursor().col)
                    .ok_or_else(|| report!("muxr composite cursor col overflowed"))?;
                cursor = RenderCursor::new(row, col, true);
            }
        }
        self::paste_borders(&mut rows, &pane_layout.borders)?;

        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(row, cells)| {
                let row = u16::try_from(row).context("muxr composite render row overflowed")?;
                Ok(RenderRowSpan::new(row, 0, cells))
            })
            .collect::<rootcause::Result<Vec<_>>>()?;

        Ok(CompositeFrame {
            cursor,
            rows,
            seq: 0,
            size: size.clone(),
        })
    }

    fn next_sequence(&mut self) -> rootcause::Result<u64> {
        let seq = self.next_seq;
        self.next_seq = self
            .next_seq
            .checked_add(1)
            .ok_or_else(|| report!("muxr composite render sequence overflowed"))?;
        Ok(seq)
    }
}

struct ServerFilesGuard {
    paths: SessionPaths,
}

impl Drop for ServerFilesGuard {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.paths.socket));
        drop(fs::remove_file(&self.paths.pid));
    }
}

struct ClientSlotGuard<'a> {
    active_client: &'a AtomicBool,
}

impl Drop for ClientSlotGuard<'_> {
    fn drop(&mut self) {
        self.active_client.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ServerInputMode {
    #[default]
    Normal,
    Resize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum KeyResolution {
    Command(ClientCommand),
    Raw,
}

/// Run the muxr server for one internally launched session.
///
/// # Errors
/// - Server startup, socket IO, PTY setup, or pid file persistence fails.
pub fn serve_session(session: &SessionName) -> rootcause::Result<()> {
    let paths = SessionPaths::from_home(session)?;

    self::serve(&ServerConfig {
        session: session.clone(),
        paths,
        max_accepted_connections: None,
        shell_command: ShellCommand::default_from_env(),
    })
}

/// Run a muxr Unix socket server for one session.
///
/// # Errors
/// - Session directories, pid file, Unix listener, or PTY setup fails.
/// - Accepted client connections fail protocol handling.
pub fn serve(config: &ServerConfig) -> rootcause::Result<()> {
    self::run_async(self::serve_async(config))
}

async fn serve_async(config: &ServerConfig) -> rootcause::Result<()> {
    if matches!(config.max_accepted_connections, Some(0)) {
        return Ok(());
    }

    self::prepare_session_dirs(&config.paths)?;
    let listener = ServerListener::bind(&config.paths.socket)?;
    // Own the socket file as soon as bind succeeds so later startup failures do not leave stale sockets.
    let _files_guard = ServerFilesGuard {
        paths: config.paths.clone(),
    };
    self::secure_socket_file(&config.paths.socket)?;
    fs::write(&config.paths.pid, std::process::id().to_string()).context("failed to write muxr server pid")?;
    let initial_size = TerminalSize::new(80, 24)?;
    let metadata = SessionMetadata::new(config)?;
    let layout = match self::load_layout_metadata(&config.paths, config)? {
        Some(layout) => layout,
        None => SessionLayout::initial(config, metadata)?,
    };
    let runtimes = PaneRuntimes::spawn_for_layout(config, &layout, &initial_size)?;
    let layout = Arc::new(Mutex::new(layout));
    let runtimes = Arc::new(Mutex::new(runtimes));
    {
        let locked_layout = self::lock_mutex(layout.as_ref(), "layout")?;
        self::write_layout_metadata(&config.paths, &locked_layout)?;
    }
    let active_client = Arc::new(AtomicBool::new(false));
    let mut accepted_connections = 0_usize;
    let mut handles = Vec::new();

    loop {
        if self::reap_exited_panes(&config.paths, &layout, &runtimes)?.final_pane_exhausted
            || self::lock_mutex(runtimes.as_ref(), "pane runtimes")?.is_empty()
        {
            break;
        }

        self::join_finished_client_tasks(&mut handles).await?;

        tokio::select! {
            accepted = listener.accept() => {
                let connection = accepted?;
                accepted_connections = accepted_connections
                    .checked_add(1)
                    .ok_or_else(|| report!("muxr accepted connection count overflowed"))?;
                self::spawn_client_task(config, &active_client, &layout, &runtimes, connection, &mut handles);

                if let Some(max_accepted_connections) = config.max_accepted_connections
                    && accepted_connections >= max_accepted_connections
                {
                    break;
                }
            }
            () = tokio::time::sleep(ACCEPT_POLL_INTERVAL) => {}
        }
    }

    self::join_client_tasks(handles).await?;
    Ok(())
}

fn prepare_session_dirs(paths: &SessionPaths) -> rootcause::Result<()> {
    let sessions_root = paths
        .root
        .parent()
        .ok_or_else(|| report!("muxr session root has no parent"))?;
    let socket_root = paths
        .socket
        .parent()
        .ok_or_else(|| report!("muxr socket path has no parent"))?;
    let state_root = socket_root
        .parent()
        .ok_or_else(|| report!("muxr socket root has no parent"))?;

    // Socket names are deterministic, so every muxr-owned directory that can expose them must be private.
    for (path, label) in [
        (state_root, "state root"),
        (sessions_root, "sessions root"),
        (socket_root, "socket root"),
        (paths.root.as_path(), "session root"),
        (paths.panes.as_path(), "panes root"),
    ] {
        self::ensure_private_dir(path, label)?;
    }

    Ok(())
}

fn ensure_private_dir(path: &Path, label: &str) -> rootcause::Result<()> {
    fs::create_dir_all(path).context(format!("failed to create muxr {label}"))?;
    let metadata = fs::symlink_metadata(path).context(format!("failed to inspect muxr {label}"))?;
    if metadata.file_type().is_symlink() {
        return Err(report!("unsafe muxr directory")
            .attach(format!("label={label}"))
            .attach("reason=symlinks are not allowed")
            .attach(format!("path={}", path.display())));
    }
    if !metadata.is_dir() {
        return Err(report!("unsafe muxr directory")
            .attach(format!("label={label}"))
            .attach("reason=path is not a directory")
            .attach(format!("path={}", path.display())));
    }

    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIR_MODE))
        .context(format!("failed to secure muxr {label} permissions"))?;
    self::validate_private_mode(path, label, PRIVATE_DIR_MODE)
}

fn secure_socket_file(path: &Path) -> rootcause::Result<()> {
    // The directory is private, but the socket itself should not be group/other accessible if copied or moved.
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_SOCKET_MODE))
        .context("failed to secure muxr socket file permissions")?;
    self::validate_private_mode(path, "socket file", PRIVATE_SOCKET_MODE)
}

fn validate_private_mode(path: &Path, label: &str, expected_mode: u32) -> rootcause::Result<()> {
    let mode = fs::metadata(path)
        .context(format!("failed to read muxr {label} permissions"))?
        .permissions()
        .mode()
        & 0o777;

    if mode & GROUP_OR_OTHER_PERMISSIONS_MASK != 0 {
        return Err(report!("unsafe muxr permissions")
            .attach(format!("label={label}"))
            .attach(format!("expected={expected_mode:o}"))
            .attach(format!("actual={mode:o}"))
            .attach(format!("path={}", path.display())));
    }

    Ok(())
}

fn write_layout_metadata(paths: &SessionPaths, layout: &SessionLayout) -> rootcause::Result<()> {
    let layout = PersistedLayout {
        version: LAYOUT_VERSION,
        session: &layout.session,
        active_tab: &layout.active_tab,
        tabs: &layout.tabs,
    };
    let encoded = serde_json::to_vec_pretty(&layout).context("failed to serialize muxr layout metadata")?;

    fs::write(&paths.layout, encoded).context("failed to write muxr layout metadata")?;
    Ok(())
}

fn load_layout_metadata(paths: &SessionPaths, config: &ServerConfig) -> rootcause::Result<Option<SessionLayout>> {
    let encoded = match fs::read(&paths.layout) {
        Ok(encoded) => encoded,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("failed to read muxr layout metadata")?,
    };
    let persisted: PersistedLayoutOwned =
        serde_json::from_slice(&encoded).context("failed to parse muxr layout metadata")?;

    Ok(Some(SessionLayout::from_persisted(config, persisted)?))
}

fn next_number<'a>(ids: impl Iterator<Item = &'a str>, prefix: &str) -> rootcause::Result<u64> {
    let max_number = ids
        .filter_map(|id| id.strip_prefix(prefix))
        .filter_map(|suffix| suffix.parse::<u64>().ok())
        .max()
        .unwrap_or(0);

    max_number
        .checked_add(1)
        .ok_or_else(|| report!("muxr layout id counter overflowed").attach(format!("prefix={prefix}")))
}

fn unix_timestamp_millis() -> rootcause::Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("failed to read system time for muxr layout metadata")?
        .as_millis();

    Ok(u64::try_from(millis).context("muxr layout metadata timestamp overflowed")?)
}

fn lock_mutex<'a, T>(mutex: &'a Mutex<T>, name: &str) -> rootcause::Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| report!("poisoned muxr {name} mutex"))
}

fn empty_render_rows(size: &TerminalSize) -> Vec<Vec<RenderCell>> {
    let blank = RenderCell::narrow(" ", RenderStyle::default());
    (0..size.rows())
        .map(|_| vec![blank.clone(); usize::from(size.cols())])
        .collect()
}

fn paste_snapshot(
    rows: &mut [Vec<RenderCell>],
    region: &PaneRegion,
    snapshot: &TerminalSnapshot,
) -> rootcause::Result<()> {
    if snapshot.size().cols() != region.cols || snapshot.size().rows() != region.rows {
        return Err(report!("muxr pane snapshot size does not match region")
            .attach(format!("pane_id={}", region.id))
            .attach(format!("snapshot_cols={}", snapshot.size().cols()))
            .attach(format!("snapshot_rows={}", snapshot.size().rows()))
            .attach(format!("region_cols={}", region.cols))
            .attach(format!("region_rows={}", region.rows)));
    }

    for span in snapshot.rows() {
        let row = region
            .row
            .checked_add(span.row)
            .ok_or_else(|| report!("muxr pane row offset overflowed"))?;
        let col = region
            .col
            .checked_add(span.col)
            .ok_or_else(|| report!("muxr pane col offset overflowed"))?;
        let target_row = rows
            .get_mut(usize::from(row))
            .ok_or_else(|| report!("muxr pane row outside composite frame"))?;
        let col = usize::from(col);
        let end_col = col
            .checked_add(span.cells.len())
            .ok_or_else(|| report!("muxr pane span end overflowed"))?;
        if end_col > target_row.len() {
            return Err(report!("muxr pane span outside composite frame").attach(format!("pane_id={}", region.id)));
        }
        for (target, cell) in target_row.iter_mut().skip(col).zip(span.cells.iter()) {
            *target = cell.clone();
        }
    }
    Ok(())
}

fn paste_borders(rows: &mut [Vec<RenderCell>], borders: &[PaneBorder]) -> rootcause::Result<()> {
    for border in borders {
        match border.axis {
            PaneBorderAxis::Horizontal => {
                let target_row = rows
                    .get_mut(usize::from(border.row))
                    .ok_or_else(|| report!("muxr horizontal pane border row outside composite frame"))?;
                let start_col = usize::from(border.col);
                let end_col = start_col
                    .checked_add(usize::from(border.len))
                    .ok_or_else(|| report!("muxr horizontal pane border end overflowed"))?;
                if end_col > target_row.len() {
                    return Err(report!("muxr horizontal pane border outside composite frame"));
                }
                for target in target_row.iter_mut().skip(start_col).take(usize::from(border.len)) {
                    self::paste_border_cell(target, "─");
                }
            }
            PaneBorderAxis::Vertical => {
                let end_row = border
                    .row
                    .checked_add(border.len)
                    .ok_or_else(|| report!("muxr vertical pane border end overflowed"))?;
                for row in border.row..end_row {
                    let target_row = rows
                        .get_mut(usize::from(row))
                        .ok_or_else(|| report!("muxr vertical pane border row outside composite frame"))?;
                    let target = target_row
                        .get_mut(usize::from(border.col))
                        .ok_or_else(|| report!("muxr vertical pane border col outside composite frame"))?;
                    self::paste_border_cell(target, "│");
                }
            }
        }
    }
    Ok(())
}

fn paste_border_cell(target: &mut RenderCell, glyph: &'static str) {
    let glyph = match (target.text.as_str(), glyph) {
        ("│", "─") | ("─", "│") | ("┼", _) => "┼",
        _ => glyph,
    };
    *target = RenderCell::narrow(glyph, self::border_style());
}

const fn border_style() -> RenderStyle {
    RenderStyle {
        attrs: RenderTextStyle::empty().set_dim(true),
        bg: RenderColor::Default,
        fg: RenderColor::Indexed(8),
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ReapResult {
    final_pane_exhausted: bool,
    layout_changed: bool,
}

struct AttachedPtySink {
    guard: PtySinkGuard,
    pane_id: PaneId,
}

struct AttachedSessionState<'a> {
    config: &'a ServerConfig,
    layout: &'a Mutex<SessionLayout>,
    pty_event_sender: &'a mpsc::SyncSender<PtyEvent>,
    render_composer: &'a mut RenderComposer,
    runtimes: &'a Mutex<PaneRuntimes>,
    sink_guards: &'a mut Vec<AttachedPtySink>,
    terminal_size: TerminalSize,
}

fn attach_pane_sinks(
    runtimes: &Mutex<PaneRuntimes>,
    sender: &mpsc::SyncSender<PtyEvent>,
) -> rootcause::Result<Vec<AttachedPtySink>> {
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    runtimes
        .panes
        .iter()
        .map(|pane| {
            Ok(AttachedPtySink {
                guard: pane.session.handle().attach_sink(sender.clone())?,
                pane_id: pane.id.clone(),
            })
        })
        .collect()
}

fn attach_pane_sink(
    runtimes: &Mutex<PaneRuntimes>,
    sender: &mpsc::SyncSender<PtyEvent>,
    pane_id: &PaneId,
) -> rootcause::Result<AttachedPtySink> {
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    Ok(AttachedPtySink {
        guard: runtimes.handle(pane_id)?.attach_sink(sender.clone())?,
        pane_id: pane_id.clone(),
    })
}

fn remove_pane_sink(sink_guards: &mut Vec<AttachedPtySink>, pane_id: &PaneId) {
    sink_guards.retain(|sink| sink.pane_id != *pane_id);
}

fn pane_sinks_are_current(sink_guards: &[AttachedPtySink]) -> bool {
    sink_guards.iter().all(|sink| sink.guard.is_output_current())
}

fn resize_panes_to_layout(
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    size: &TerminalSize,
) -> rootcause::Result<()> {
    let regions = {
        let layout = self::lock_mutex(layout, "layout")?;
        layout.pane_regions(size)?
    };
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    runtimes.resize_panes(&regions)
}

fn reap_exited_panes(
    paths: &SessionPaths,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<ReapResult> {
    let exited_panes = {
        let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
        runtimes.exited_panes()?
    };
    if exited_panes.is_empty() {
        return Ok(ReapResult::default());
    }

    let exited_at = self::unix_timestamp_millis()?;
    let mut final_pane_exhausted = false;
    {
        let mut layout = self::lock_mutex(layout, "layout")?;
        let mut removed_panes = Vec::new();
        for (pane_id, exit_status) in &exited_panes {
            match layout.remove_exited_pane(pane_id, exited_at, exit_status.clone())? {
                PaneExitOutcome::Final => final_pane_exhausted = true,
                PaneExitOutcome::Removed => {}
            }
            removed_panes.push(pane_id.clone());
        }
        self::write_layout_metadata(paths, &layout)?;
        drop(layout);

        let mut runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
        for pane_id in removed_panes {
            runtimes.remove(&pane_id);
        }
        drop(runtimes);
    }

    Ok(ReapResult {
        final_pane_exhausted,
        layout_changed: true,
    })
}

fn spawn_client_task(
    config: &ServerConfig,
    active_client: &Arc<AtomicBool>,
    layout: &Arc<Mutex<SessionLayout>>,
    runtimes: &Arc<Mutex<PaneRuntimes>>,
    connection: ServerConnection,
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) {
    let active_client = Arc::clone(active_client);
    let config = config.clone();
    let layout = Arc::clone(layout);
    let runtimes = Arc::clone(runtimes);
    handles.push(tokio::spawn(async move {
        self::handle_client(&config, connection, &active_client, &layout, &runtimes).await
    }));
}

async fn handle_client(
    config: &ServerConfig,
    mut connection: ServerConnection,
    active_client: &AtomicBool,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<()> {
    let Ok(Ok(Some(request))) = tokio::time::timeout(CLIENT_HANDSHAKE_TIMEOUT, connection.recv_request()).await else {
        return Ok(());
    };

    let hello = match request {
        ClientRequest::Ping => {
            let _sent = self::send_connection_event_with_timeout(&mut connection, &ServerEvent::Pong).await?;
            return Ok(());
        }
        ClientRequest::Hello(hello) => hello,
        request @ (ClientRequest::Pong
        | ClientRequest::Detach
        | ClientRequest::RenderResync
        | ClientRequest::Resize(_)
        | ClientRequest::Input(_)
        | ClientRequest::Paste(_)
        | ClientRequest::Key(_)
        | ClientRequest::ScrollPaneAt { .. }
        | ClientRequest::FocusPaneAt(_)) => {
            let _sent = self::send_connection_event_with_timeout(
                &mut connection,
                &ServerEvent::Error(ServerError::unexpected_request(&request)),
            )
            .await?;
            return Ok(());
        }
    };

    if hello.protocol_version != PROTOCOL_VERSION {
        let _sent = self::send_connection_event_with_timeout(
            &mut connection,
            &ServerEvent::Error(ServerError::protocol_version_mismatch(hello.protocol_version)),
        )
        .await?;
        return Ok(());
    }

    if active_client.swap(true, Ordering::AcqRel) {
        let _sent = self::send_connection_event_with_timeout(
            &mut connection,
            &ServerEvent::Error(ServerError::client_already_attached()),
        )
        .await?;
        return Ok(());
    }
    let _client_slot_guard = ClientSlotGuard { active_client };

    if hello.session != config.session {
        let _sent = self::send_connection_event_with_timeout(
            &mut connection,
            &ServerEvent::Error(ServerError::session_mismatch(&config.session, &hello.session)),
        )
        .await?;
        return Ok(());
    }

    self::resize_panes_to_layout(layout, runtimes, &hello.terminal_size)?;
    let (pty_event_sender, pty_event_receiver) = mpsc::sync_channel(OUTPUT_EVENT_CHANNEL_LIMIT);
    let mut sink_guards = self::attach_pane_sinks(runtimes, &pty_event_sender)?;
    let (mut request_reader, mut event_writer) = connection.split();
    let (layout_snapshot, mut render_composer, render_baseline) =
        self::initial_attached_render(layout, runtimes, &hello.terminal_size)?;
    if !self::send_attached_hello_and_baseline(&mut event_writer, config, layout_snapshot, render_baseline).await? {
        return Ok(());
    }

    let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(OUTPUT_EVENT_CHANNEL_LIMIT);
    let bridge_handle = tokio::task::spawn_blocking(move || {
        while let Ok(event) = pty_event_receiver.recv() {
            if async_pty_sender.blocking_send(event).is_err() {
                break;
            }
        }
    });
    let mut attached_state = AttachedSessionState {
        config,
        layout,
        pty_event_sender: &pty_event_sender,
        render_composer: &mut render_composer,
        runtimes,
        sink_guards: &mut sink_guards,
        terminal_size: hello.terminal_size,
    };
    let result = self::run_attached_client(
        &mut request_reader,
        &mut event_writer,
        &mut attached_state,
        &mut async_pty_receiver,
    )
    .await;

    drop(sink_guards);
    drop(pty_event_sender);
    drop(async_pty_receiver);
    bridge_handle
        .await
        .map_err(|error| report!("muxr server pty bridge task panicked").attach(format!("{error}")))?;
    result
}

fn initial_attached_render(
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<(LayoutSnapshot, RenderComposer, RenderUpdate)> {
    let mut render_composer = RenderComposer::new();
    let layout = self::lock_mutex(layout, "layout")?;
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    let layout_snapshot = layout.snapshot()?;
    let render_baseline = render_composer.render_baseline(&layout, &runtimes, terminal_size)?;
    drop(runtimes);
    drop(layout);
    Ok((layout_snapshot, render_composer, render_baseline))
}

async fn send_attached_hello_and_baseline(
    event_writer: &mut ServerEventWriter,
    config: &ServerConfig,
    layout: LayoutSnapshot,
    render_baseline: RenderUpdate,
) -> rootcause::Result<bool> {
    if !self::send_writer_event_with_timeout(
        event_writer,
        &ServerEvent::Hello(ServerHello {
            protocol_version: PROTOCOL_VERSION,
            session: config.session.clone(),
            server_pid: ServerPid::new(std::process::id())?,
            layout,
        }),
    )
    .await?
    {
        return Ok(false);
    }
    self::send_writer_event_with_timeout(event_writer, &ServerEvent::Render(render_baseline)).await
}

async fn run_attached_client(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<PtyEvent>,
) -> rootcause::Result<()> {
    let mut shell_poll = tokio::time::interval(CLIENT_EVENT_POLL_INTERVAL);
    let heartbeat_start = tokio::time::Instant::now()
        .checked_add(CLIENT_HEARTBEAT_INTERVAL)
        .ok_or_else(|| report!("muxr heartbeat interval overflowed"))?;
    let mut heartbeat = tokio::time::interval_at(heartbeat_start, CLIENT_HEARTBEAT_INTERVAL);
    let mut heartbeat_started_at: Option<tokio::time::Instant> = None;
    let mut input_mode = ServerInputMode::Normal;
    let render_start = tokio::time::Instant::now()
        .checked_add(RENDER_FRAME_INTERVAL)
        .ok_or_else(|| report!("muxr render frame interval overflowed"))?;
    let mut render_tick = tokio::time::interval_at(render_start, RENDER_FRAME_INTERVAL);
    let mut render_dirty = false;
    let mut request_turn = false;

    loop {
        // A dropped PTY sink means live output is already stale; release the
        // active slot instead of draining old frames into a slow client.
        if !self::pane_sinks_are_current(state.sink_guards) {
            return Ok(());
        }
        if let Some(started_at) = heartbeat_started_at
            && started_at.elapsed() > CLIENT_HEARTBEAT_TIMEOUT
        {
            return Ok(());
        }

        if request_turn {
            tokio::select! {
                biased;
                _ = heartbeat.tick() => {
                    if heartbeat_started_at.is_none() {
                        if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Ping).await? {
                            return Ok(());
                        }
                        heartbeat_started_at = Some(tokio::time::Instant::now());
                    }
                },
                _ = shell_poll.tick() => {
                    if self::handle_reaped_panes(state, event_writer).await? {
                        return Ok(());
                    }
                },
                _ = render_tick.tick() => {
                    if !self::flush_render_diff(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    if !self::handle_attached_request(request?, event_writer, state, &mut heartbeat_started_at, &mut input_mode, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    request_turn = true;
                    if !self::handle_pty_event(event, event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
            }
        } else {
            tokio::select! {
                biased;
                _ = heartbeat.tick() => {
                    if heartbeat_started_at.is_none() {
                        if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Ping).await? {
                            return Ok(());
                        }
                        heartbeat_started_at = Some(tokio::time::Instant::now());
                    }
                },
                _ = shell_poll.tick() => {
                    if self::handle_reaped_panes(state, event_writer).await? {
                        return Ok(());
                    }
                },
                _ = render_tick.tick() => {
                    if !self::flush_render_diff(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    // Output gets one turn, then client requests get first chance so detach/pong cannot starve.
                    request_turn = true;
                    if !self::handle_pty_event(event, event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    if !self::handle_attached_request(request?, event_writer, state, &mut heartbeat_started_at, &mut input_mode, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
            }
        }
    }
}

async fn handle_pty_event(
    event: Option<PtyEvent>,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match event {
        Some(PtyEvent::Exited) => Ok(!self::handle_reaped_panes(state, event_writer).await?),
        Some(PtyEvent::Output) => {
            *render_dirty = true;
            Ok(true)
        }
        None => Ok(false),
    }
}

async fn flush_render_diff(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    if !*render_dirty {
        return Ok(true);
    }

    let update = {
        let layout = self::lock_mutex(state.layout, "layout")?;
        let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
        state
            .render_composer
            .render_diff(&layout, &runtimes, &state.terminal_size)?
    };
    if let Some(update) = update
        && !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Render(update)).await?
    {
        return Ok(false);
    }
    *render_dirty = false;
    Ok(true)
}

async fn send_layout_and_baseline(
    event_writer: &mut ServerEventWriter,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    render_composer: &mut RenderComposer,
    terminal_size: &TerminalSize,
) -> rootcause::Result<bool> {
    let (layout_snapshot, render_update) = {
        let layout = self::lock_mutex(layout, "layout")?;
        let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
        (
            layout.snapshot()?,
            render_composer.render_baseline(&layout, &runtimes, terminal_size)?,
        )
    };
    if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Layout(layout_snapshot)).await? {
        return Ok(false);
    }
    self::send_writer_event_with_timeout(event_writer, &ServerEvent::Render(render_update)).await
}

async fn handle_reaped_panes(
    state: &mut AttachedSessionState<'_>,
    event_writer: &mut ServerEventWriter,
) -> rootcause::Result<bool> {
    let reap = self::reap_exited_panes(&state.config.paths, state.layout, state.runtimes)?;
    if reap.final_pane_exhausted {
        return Ok(true);
    }
    if reap.layout_changed {
        let live_panes = {
            let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
            runtimes.panes.iter().map(|pane| pane.id.clone()).collect::<Vec<_>>()
        };
        state.sink_guards.retain(|sink| live_panes.contains(&sink.pane_id));
        if !self::resize_panes_and_render(event_writer, state).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn resize_panes_and_render(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    self::resize_panes_to_layout(state.layout, state.runtimes, &state.terminal_size)?;
    self::send_layout_and_baseline(
        event_writer,
        state.layout,
        state.runtimes,
        state.render_composer,
        &state.terminal_size,
    )
    .await
}

async fn handle_attached_request(
    request: Option<ClientRequest>,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    input_mode: &mut ServerInputMode,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match request {
        Some(ClientRequest::Detach) => {
            let _sent = self::send_writer_event_with_timeout(event_writer, &ServerEvent::Detached).await?;
            Ok(false)
        }
        Some(ClientRequest::Input(bytes)) => {
            if self::active_pane_handle(state.layout, state.runtimes)?.write_input(&bytes)? {
                *render_dirty = true;
            }
            Ok(true)
        }
        Some(ClientRequest::Paste(bytes)) => {
            if self::active_pane_handle(state.layout, state.runtimes)?.write_paste(&bytes)? {
                *render_dirty = true;
            }
            Ok(true)
        }
        Some(ClientRequest::Key(key)) => {
            self::handle_key_request(key, event_writer, state, input_mode, render_dirty).await
        }
        Some(ClientRequest::ScrollPaneAt { position, direction }) => {
            // Wheel packets include their own coordinates, so route scrollback by pointer position without stealing
            // keyboard focus from the active pane.
            if !self::handle_scroll_pane_at_request(
                position,
                direction,
                state.layout,
                state.runtimes,
                &state.terminal_size,
            )? {
                return Ok(true);
            }
            // Wheel input can arrive much faster than render IO; mark the pane dirty and let the normal render tick
            // coalesce many scroll offsets into one diff.
            *render_dirty = true;
            Ok(true)
        }
        Some(ClientRequest::FocusPaneAt(position)) => {
            if !self::handle_focus_pane_at_request(position, state.config, state.layout, &state.terminal_size)? {
                return Ok(true);
            }
            if !self::send_layout_and_baseline(
                event_writer,
                state.layout,
                state.runtimes,
                state.render_composer,
                &state.terminal_size,
            )
            .await?
            {
                return Ok(false);
            }
            Ok(true)
        }
        Some(ClientRequest::Resize(size)) => {
            state.terminal_size = size;
            if !self::resize_panes_and_render(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        Some(ClientRequest::RenderResync) => {
            if !self::send_layout_and_baseline(
                event_writer,
                state.layout,
                state.runtimes,
                state.render_composer,
                &state.terminal_size,
            )
            .await?
            {
                return Ok(false);
            }
            Ok(true)
        }
        Some(ClientRequest::Ping) => self::send_writer_event_with_timeout(event_writer, &ServerEvent::Pong).await,
        Some(ClientRequest::Pong) => {
            *heartbeat_started_at = None;
            Ok(true)
        }
        Some(request @ ClientRequest::Hello(_)) => {
            let _sent = self::send_writer_event_with_timeout(
                event_writer,
                &ServerEvent::Error(ServerError::unexpected_request(&request)),
            )
            .await?;
            Ok(false)
        }
        None => Ok(false),
    }
}

async fn handle_key_request(
    key: ClientKey,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    input_mode: &mut ServerInputMode,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match self::resolve_key(input_mode, &key) {
        KeyResolution::Command(command) => self::handle_command_request(command, event_writer, state).await,
        KeyResolution::Raw => {
            if self::active_pane_handle(state.layout, state.runtimes)?.write_input(&key.raw_bytes)? {
                *render_dirty = true;
            }
            Ok(true)
        }
    }
}

async fn handle_command_request(
    command: ClientCommand,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    match command {
        ClientCommand::CreateTab
        | ClientCommand::FocusPreviousTab
        | ClientCommand::FocusNextTab
        | ClientCommand::MoveTabPrevious
        | ClientCommand::MoveTabNext => {
            if let Some(pane_id) = self::handle_tab_command(
                command,
                state.config,
                state.layout,
                state.runtimes,
                &state.terminal_size,
            )? {
                state.sink_guards.push(self::attach_pane_sink(
                    state.runtimes,
                    state.pty_event_sender,
                    &pane_id,
                )?);
            }
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCommand::SplitPaneHorizontal | ClientCommand::SplitPaneVertical => {
            let pane_id = self::handle_split_pane_command(
                command,
                state.config,
                state.layout,
                state.runtimes,
                &state.terminal_size,
            )?;
            state.sink_guards.push(self::attach_pane_sink(
                state.runtimes,
                state.pty_event_sender,
                &pane_id,
            )?);
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCommand::ClosePane => {
            let outcome = self::handle_close_pane_command(state.config, state.layout, state.runtimes)?;
            self::remove_pane_sink(state.sink_guards, &outcome.pane_id);
            if outcome.final_pane {
                let _sent = self::send_writer_event_with_timeout(event_writer, &ServerEvent::Detached).await?;
                return Ok(false);
            }
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCommand::ResizePane(direction) => {
            if !self::handle_resize_pane_command(direction, state.config, state.layout)? {
                return Ok(true);
            }
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCommand::FocusPane(direction) => {
            if !self::handle_focus_pane_command(direction, state.config, state.layout, &state.terminal_size)? {
                return Ok(true);
            }
            self::send_layout_and_baseline(
                event_writer,
                state.layout,
                state.runtimes,
                state.render_composer,
                &state.terminal_size,
            )
            .await
        }
        ClientCommand::EnterResizeMode | ClientCommand::ExitMode => Ok(true),
    }
}

fn active_pane_handle(layout: &Mutex<SessionLayout>, runtimes: &Mutex<PaneRuntimes>) -> rootcause::Result<PtyHandle> {
    let active_pane = {
        let layout = self::lock_mutex(layout, "layout")?;
        layout.active_pane_id()?
    };
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    runtimes.handle(&active_pane)
}

fn handle_tab_command(
    command: ClientCommand,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<Option<PaneId>> {
    let mut layout = self::lock_mutex(layout, "layout")?;
    let pane_id = match command {
        ClientCommand::CreateTab => {
            let pane_id = layout.create_tab(SessionMetadata::new(config)?)?;
            let mut runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
            runtimes.spawn_pane(pane_id.clone(), config, terminal_size)?;
            drop(runtimes);
            Some(pane_id)
        }
        ClientCommand::FocusPreviousTab => {
            layout.focus_previous_tab()?;
            None
        }
        ClientCommand::FocusNextTab => {
            layout.focus_next_tab()?;
            None
        }
        ClientCommand::MoveTabPrevious => {
            layout.move_active_tab_previous()?;
            None
        }
        ClientCommand::MoveTabNext => {
            layout.move_active_tab_next()?;
            None
        }
        command @ (ClientCommand::ClosePane
        | ClientCommand::EnterResizeMode
        | ClientCommand::ExitMode
        | ClientCommand::FocusPane(_)
        | ClientCommand::ResizePane(_)
        | ClientCommand::SplitPaneHorizontal
        | ClientCommand::SplitPaneVertical) => {
            return Err(report!("muxr non-tab command reached tab handler").attach(format!("{command:?}")));
        }
    };
    self::write_layout_metadata(&config.paths, &layout)?;
    drop(layout);
    Ok(pane_id)
}

fn handle_split_pane_command(
    command: ClientCommand,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneId> {
    let split_axis = match command {
        ClientCommand::SplitPaneHorizontal => PaneSplitAxis::Horizontal,
        ClientCommand::SplitPaneVertical => PaneSplitAxis::Vertical,
        command @ (ClientCommand::ClosePane
        | ClientCommand::CreateTab
        | ClientCommand::EnterResizeMode
        | ClientCommand::ExitMode
        | ClientCommand::FocusPane(_)
        | ClientCommand::FocusNextTab
        | ClientCommand::FocusPreviousTab
        | ClientCommand::MoveTabNext
        | ClientCommand::MoveTabPrevious
        | ClientCommand::ResizePane(_)) => {
            return Err(report!("muxr non-split command reached split handler").attach(format!("{command:?}")));
        }
    };
    let mut layout = self::lock_mutex(layout, "layout")?;
    let pane_id = layout.split_active_pane(SessionMetadata::new(config)?, split_axis)?;
    {
        let mut runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
        runtimes.spawn_pane(pane_id.clone(), config, terminal_size)?;
        drop(runtimes);
    }
    self::write_layout_metadata(&config.paths, &layout)?;
    drop(layout);
    Ok(pane_id)
}

fn handle_close_pane_command(
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<ClosePaneOutcome> {
    let exited_at = self::unix_timestamp_millis()?;
    let mut layout = self::lock_mutex(layout, "layout")?;
    let outcome = layout.close_active_pane(exited_at)?;
    {
        let mut runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
        runtimes.remove(&outcome.pane_id);
        drop(runtimes);
    }
    self::write_layout_metadata(&config.paths, &layout)?;
    drop(layout);
    Ok(outcome)
}

fn handle_resize_pane_command(
    direction: PaneResizeDirection,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
) -> rootcause::Result<bool> {
    let mut layout = self::lock_mutex(layout, "layout")?;
    let resized = layout.resize_active_pane(direction)?;
    if resized {
        self::write_layout_metadata(&config.paths, &layout)?;
    }
    drop(layout);
    Ok(resized)
}

fn handle_focus_pane_command(
    direction: PaneFocusDirection,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<bool> {
    let mut layout = self::lock_mutex(layout, "layout")?;
    let focused = layout.focus_pane_direction(terminal_size, direction)?;
    if focused {
        self::write_layout_metadata(&config.paths, &layout)?;
    }
    drop(layout);
    Ok(focused)
}

fn handle_focus_pane_at_request(
    position: ClientMousePosition,
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<bool> {
    let mut layout = self::lock_mutex(layout, "layout")?;
    let focused = layout.focus_pane_at(terminal_size, position)?;
    if focused {
        self::write_layout_metadata(&config.paths, &layout)?;
    }
    drop(layout);
    Ok(focused)
}

fn handle_scroll_pane_at_request(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<bool> {
    let pane_id = {
        let layout = self::lock_mutex(layout, "layout")?;
        let pane_id = layout.pane_at(terminal_size, position)?;
        drop(layout);
        let Some(pane_id) = pane_id else {
            return Ok(false);
        };
        pane_id
    };

    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    runtimes.handle(&pane_id)?.scroll(direction)
}

const fn resolve_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match input_mode {
        ServerInputMode::Normal => self::resolve_normal_key(input_mode, key),
        ServerInputMode::Resize => self::resolve_resize_key(input_mode, key),
    }
}

const fn resolve_normal_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match (&key.code, key.modifiers) {
        (ClientKeyCode::Char('E'), ClientKeyModifiers::SHIFT_ALT) => KeyResolution::Command(ClientCommand::CreateTab),
        (ClientKeyCode::Char('P'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Command(ClientCommand::FocusPreviousTab)
        }
        (ClientKeyCode::Char('N'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Command(ClientCommand::FocusNextTab)
        }
        (ClientKeyCode::Char('p'), ClientKeyModifiers::CTRL_ALT) => {
            KeyResolution::Command(ClientCommand::MoveTabPrevious)
        }
        (ClientKeyCode::Char('n'), ClientKeyModifiers::CTRL_ALT) => KeyResolution::Command(ClientCommand::MoveTabNext),
        (ClientKeyCode::Char('H'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Command(ClientCommand::FocusPane(PaneFocusDirection::Left))
        }
        (ClientKeyCode::Char('J'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Command(ClientCommand::FocusPane(PaneFocusDirection::Down))
        }
        (ClientKeyCode::Char('K'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Command(ClientCommand::FocusPane(PaneFocusDirection::Up))
        }
        (ClientKeyCode::Char('L'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Command(ClientCommand::FocusPane(PaneFocusDirection::Right))
        }
        (ClientKeyCode::Char('V'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Command(ClientCommand::SplitPaneVertical)
        }
        (ClientKeyCode::Char('D'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Command(ClientCommand::SplitPaneHorizontal)
        }
        (ClientKeyCode::Char('W'), ClientKeyModifiers::SHIFT_ALT) => KeyResolution::Command(ClientCommand::ClosePane),
        (ClientKeyCode::Char('R'), ClientKeyModifiers::SHIFT_ALT) => {
            *input_mode = ServerInputMode::Resize;
            KeyResolution::Command(ClientCommand::EnterResizeMode)
        }
        _ => KeyResolution::Raw,
    }
}

const fn resolve_resize_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match (&key.code, key.modifiers) {
        (ClientKeyCode::Esc, ClientKeyModifiers::NONE) => {
            *input_mode = ServerInputMode::Normal;
            KeyResolution::Command(ClientCommand::ExitMode)
        }
        (ClientKeyCode::Char('h') | ClientKeyCode::Left, ClientKeyModifiers::NONE) => {
            KeyResolution::Command(ClientCommand::ResizePane(PaneResizeDirection::Left))
        }
        (ClientKeyCode::Char('j') | ClientKeyCode::Down, ClientKeyModifiers::NONE) => {
            KeyResolution::Command(ClientCommand::ResizePane(PaneResizeDirection::Down))
        }
        (ClientKeyCode::Char('k') | ClientKeyCode::Up, ClientKeyModifiers::NONE) => {
            KeyResolution::Command(ClientCommand::ResizePane(PaneResizeDirection::Up))
        }
        (ClientKeyCode::Char('l') | ClientKeyCode::Right, ClientKeyModifiers::NONE) => {
            KeyResolution::Command(ClientCommand::ResizePane(PaneResizeDirection::Right))
        }
        _ => KeyResolution::Raw,
    }
}

async fn send_connection_event_with_timeout(
    connection: &mut ServerConnection,
    event: &ServerEvent,
) -> rootcause::Result<bool> {
    match tokio::time::timeout(CLIENT_WRITE_TIMEOUT, connection.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}

async fn send_writer_event_with_timeout(
    writer: &mut ServerEventWriter,
    event: &ServerEvent,
) -> rootcause::Result<bool> {
    match tokio::time::timeout(CLIENT_WRITE_TIMEOUT, writer.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}

async fn join_client_tasks(handles: Vec<tokio::task::JoinHandle<rootcause::Result<()>>>) -> rootcause::Result<()> {
    for handle in handles {
        self::join_client_task(handle).await?;
    }
    Ok(())
}

async fn join_client_task(handle: tokio::task::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
    handle
        .await
        .unwrap_or_else(|error| Err(report!("muxr server client task panicked").attach(format!("{error}"))))
}

async fn join_finished_client_tasks(
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) -> rootcause::Result<()> {
    let mut pending_handles = Vec::new();
    for handle in handles.drain(..) {
        if handle.is_finished() {
            self::join_client_task(handle).await?;
        } else {
            pending_handles.push(handle);
        }
    }
    *handles = pending_handles;
    Ok(())
}

fn run_async<T>(future: impl std::future::Future<Output = rootcause::Result<T>>) -> rootcause::Result<T> {
    tokio::runtime::Runtime::new()
        .context("failed to build muxr tokio runtime")?
        .block_on(future)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::thread;
    use std::time::Instant;

    use muxr_core::ClientHello;
    use muxr_core::ClientKey;
    use muxr_core::ClientKeyCode;
    use muxr_core::ClientKeyModifiers;
    use muxr_core::RenderRowSpan;
    use muxr_core::RenderUpdate;
    use muxr_transport::ClientConnection;
    use muxr_transport::ClientEventReader;
    use muxr_transport::ClientRequestWriter;

    use super::*;

    const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(2);

    type PaneRegionSnapshot = (String, u16, u16, u16, u16);

    struct AttachedTestClient {
        layout: LayoutSnapshot,
        reader: ClientEventReader,
        server_pid: ServerPid,
        writer: ClientRequestWriter,
    }

    #[test]
    fn test_serve_when_started_creates_session_root_socket_and_pid() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            self::make_public_session_dirs(&paths)?;
            let handle = self::spawn_test_server(&session, &paths, 1);

            self::wait_for_socket(&paths.socket)?;
            self::wait_for_path(&paths.layout)?;

            assert2::assert!(paths.root.is_dir());
            assert2::assert!(paths.panes.is_dir());
            assert2::assert!(paths.layout.exists());
            assert2::assert!(paths.socket.exists());
            assert2::assert!(paths.pid.exists());
            self::assert_session_paths_are_private(&paths)?;

            self::attach_and_detach(&session, &paths).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_disconnects_accepts_future_attach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            drop(self::open_attached_client(&session, &paths).await?);
            tokio::time::sleep(Duration::from_millis(25)).await;

            pretty_assertions::assert_eq!(
                self::attach_and_detach(&session, &paths).await?,
                ServerPid::new(std::process::id())?,
            );

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_reattached_reports_same_server_pid() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;

            let first_pid = self::attach_and_detach(&session, &paths).await?;
            let second_pid = self::attach_and_detach(&session, &paths).await?;

            pretty_assertions::assert_eq!(second_pid, first_pid);
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_attached_reports_current_layout_snapshot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 1);

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::hello_request(&session)?).await?;
            let Some(ServerEvent::Hello(hello)) = connection.recv_event().await? else {
                return Err(report!("expected server hello"));
            };

            pretty_assertions::assert_eq!(hello.layout.active_tab.as_ref(), "tab-1");
            let Some(tab) = hello.layout.tabs.first() else {
                return Err(report!("expected one tab in layout snapshot"));
            };
            pretty_assertions::assert_eq!(tab.id.as_ref(), "tab-1");
            pretty_assertions::assert_eq!(tab.active_pane.as_ref(), "pane-1");
            let Some(pane) = tab.panes.first() else {
                return Err(report!("expected one pane in layout snapshot"));
            };
            pretty_assertions::assert_eq!(pane.id.as_ref(), "pane-1");

            connection.send_request(&ClientRequest::Detach).await?;
            self::read_connection_until_detached(&mut connection).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_second_client_attaches_rejects_it() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut first_client = self::open_attached_client(&session, &paths).await?;
            let mut second_client = self::connect_client(&paths).await?;

            second_client.send_request(&self::hello_request(&session)?).await?;
            let Some(ServerEvent::Error(error)) = second_client.recv_event().await? else {
                return Err(report!("expected second attach rejection"));
            };

            assert2::assert!(matches!(error, ServerError::ClientAlreadyAttached(_)));
            first_client.writer.send_request(&ClientRequest::Detach).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_never_sends_hello_does_not_occupy_attach_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let idle_client = self::connect_client(&paths).await?;

            pretty_assertions::assert_eq!(
                self::attach_and_detach(&session, &paths).await?,
                ServerPid::new(std::process::id())?,
            );

            drop(idle_client);
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_attached_client_does_not_answer_heartbeat_releases_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut stuck_client = self::connect_client(&paths).await?;
            stuck_client.send_request(&self::hello_request(&session)?).await?;
            tokio::time::sleep(
                CLIENT_HEARTBEAT_INTERVAL
                    + CLIENT_HEARTBEAT_TIMEOUT
                    + CLIENT_WRITE_TIMEOUT
                    + Duration::from_millis(100),
            )
            .await;

            let responsive_client = self::open_attached_client(&session, &paths).await?;
            self::detach_client(responsive_client).await?;

            drop(stuck_client);
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_protocol_version_mismatches_returns_structured_error() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 1);

            self::wait_for_socket(&paths.socket)?;

            let mut connection = self::connect_client(&paths).await?;
            connection
                .send_request(&ClientRequest::Hello(ClientHello {
                    protocol_version: PROTOCOL_VERSION.saturating_add(1),
                    session,
                    terminal_size: self::terminal_size()?,
                }))
                .await?;

            let Some(ServerEvent::Error(error)) = connection.recv_event().await? else {
                return Err(report!("expected protocol version mismatch error"));
            };

            assert2::assert!(matches!(error, ServerError::ProtocolVersionMismatch(_)));
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_ping_is_first_request_returns_pong_without_claiming_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut probe = self::connect_client(&paths).await?;
            probe.send_request(&ClientRequest::Ping).await?;
            pretty_assertions::assert_eq!(probe.recv_event().await?, Some(ServerEvent::Pong));

            pretty_assertions::assert_eq!(
                self::attach_and_detach(&session, &paths).await?,
                ServerPid::new(std::process::id())?,
            );
            self::join_server(handle)
        })
    }

    #[test]
    fn test_session_layout_tab_commands_when_tabs_exist_mutates_active_tab_and_order() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        layout.create_tab(self::metadata("sh", 2))?;
        layout.create_tab(self::metadata("sh", 3))?;
        pretty_assertions::assert_eq!(self::layout_tab_ids(&layout), vec!["tab-1", "tab-2", "tab-3"]);
        pretty_assertions::assert_eq!(layout.active_tab.as_ref(), "tab-3");

        layout.focus_previous_tab()?;
        pretty_assertions::assert_eq!(layout.active_tab.as_ref(), "tab-2");
        layout.move_active_tab_previous()?;
        pretty_assertions::assert_eq!(self::layout_tab_ids(&layout), vec!["tab-2", "tab-1", "tab-3"]);
        pretty_assertions::assert_eq!(layout.active_tab.as_ref(), "tab-2");
        layout.move_active_tab_next()?;
        pretty_assertions::assert_eq!(self::layout_tab_ids(&layout), vec!["tab-1", "tab-2", "tab-3"]);
        layout.focus_next_tab()?;
        pretty_assertions::assert_eq!(layout.active_tab.as_ref(), "tab-3");
        Ok(())
    }

    #[test]
    fn test_session_layout_split_and_close_when_multiple_panes_updates_active_pane() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        let pane_id = layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;

        pretty_assertions::assert_eq!(pane_id.as_ref(), "pane-2");
        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), "pane-2");
        pretty_assertions::assert_eq!(self::layout_active_tab_pane_ids(&layout)?, vec!["pane-1", "pane-2"]);

        let close = layout.close_active_pane(3)?;

        pretty_assertions::assert_eq!(close.pane_id.as_ref(), "pane-2");
        assert2::assert!(!close.final_pane);
        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), "pane-1");
        pretty_assertions::assert_eq!(self::layout_active_tab_pane_ids(&layout)?, vec!["pane-1"]);
        Ok(())
    }

    #[rstest::rstest]
    #[case::first_pane(ClientMousePosition::new(0, 0), "pane-1", true)]
    #[case::border(ClientMousePosition::new(0, 40), "pane-2", false)]
    #[case::second_pane(ClientMousePosition::new(0, 41), "pane-2", false)]
    fn test_session_layout_focus_pane_at_when_mouse_position_arrives_updates_active_pane(
        #[case] position: ClientMousePosition,
        #[case] expected_active_pane: &str,
        #[case] expected_changed: bool,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;

        pretty_assertions::assert_eq!(
            layout.focus_pane_at(&TerminalSize::new(80, 24)?, position)?,
            expected_changed,
        );
        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), expected_active_pane);
        Ok(())
    }

    #[rstest::rstest]
    #[case::first_pane(ClientMousePosition::new(0, 0), Some("pane-1"))]
    #[case::border(ClientMousePosition::new(0, 40), None)]
    #[case::second_pane(ClientMousePosition::new(0, 41), Some("pane-2"))]
    fn test_session_layout_pane_at_when_mouse_position_arrives_returns_pane_without_focus_change(
        #[case] position: ClientMousePosition,
        #[case] expected_pane: Option<&str>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;

        let pane_id = layout.pane_at(&TerminalSize::new(80, 24)?, position)?;

        pretty_assertions::assert_eq!(pane_id.as_ref().map(std::convert::AsRef::as_ref), expected_pane);
        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), "pane-2");
        Ok(())
    }

    #[rstest::rstest]
    #[case::left(PaneFocusDirection::Left, "pane-1", true)]
    #[case::right_edge(PaneFocusDirection::Right, "pane-2", false)]
    #[case::up_edge(PaneFocusDirection::Up, "pane-2", false)]
    #[case::down_edge(PaneFocusDirection::Down, "pane-2", false)]
    fn test_session_layout_focus_pane_direction_when_adjacent_pane_exists_updates_active_pane(
        #[case] direction: PaneFocusDirection,
        #[case] expected_active_pane: &str,
        #[case] expected_changed: bool,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;

        pretty_assertions::assert_eq!(
            layout.focus_pane_direction(&TerminalSize::new(80, 24)?, direction)?,
            expected_changed,
        );
        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), expected_active_pane);
        Ok(())
    }

    #[test]
    fn test_session_layout_focus_pane_direction_when_multiple_adjacent_panes_exist_uses_recent_focus()
    -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(self::metadata("sh", 3), PaneSplitAxis::Horizontal)?;

        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Up)?);
        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), "pane-2");
        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Left)?);
        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), "pane-1");

        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Right)?);

        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), "pane-2");
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical_then_horizontal(
        PaneSplitAxis::Vertical,
        PaneSplitAxis::Horizontal,
        vec![
            ("pane-1", 0, 0, 40, 24),
            ("pane-2", 41, 0, 39, 12),
            ("pane-3", 41, 13, 39, 11),
        ],
    )]
    #[case::horizontal_then_vertical(
        PaneSplitAxis::Horizontal,
        PaneSplitAxis::Vertical,
        vec![
            ("pane-1", 0, 0, 80, 12),
            ("pane-2", 0, 13, 40, 11),
            ("pane-3", 41, 13, 39, 11),
        ],
    )]
    fn test_session_layout_split_when_nested_splits_only_active_pane(
        #[case] first_axis: PaneSplitAxis,
        #[case] second_axis: PaneSplitAxis,
        #[case] expected_regions: Vec<(&str, u16, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), first_axis)?;
        layout.split_active_pane(self::metadata("sh", 3), second_axis)?;

        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), "pane-3");
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_ids(&layout)?,
            vec!["pane-1", "pane-2", "pane-3"]
        );
        let expected_regions = expected_regions
            .into_iter()
            .map(|(id, col, row, cols, rows)| (id.to_owned(), col, row, cols, rows))
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            expected_regions
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical(
        PaneSplitAxis::Vertical,
        vec![(PaneBorderAxis::Vertical, 40, 0, 24)],
    )]
    #[case::horizontal(
        PaneSplitAxis::Horizontal,
        vec![(PaneBorderAxis::Horizontal, 0, 12, 80)],
    )]
    fn test_session_layout_split_when_split_exists_reserves_border_cell(
        #[case] split_axis: PaneSplitAxis,
        #[case] expected_borders: Vec<(PaneBorderAxis, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), split_axis)?;

        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_borders(&layout, &TerminalSize::new(80, 24)?)?,
            expected_borders
        );
        Ok(())
    }

    #[test]
    fn test_paste_borders_when_borders_are_rendered_uses_box_drawing_style() -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);

        self::paste_borders(
            &mut rows,
            &[
                PaneBorder {
                    axis: PaneBorderAxis::Vertical,
                    col: 1,
                    len: 3,
                    row: 0,
                },
                PaneBorder {
                    axis: PaneBorderAxis::Horizontal,
                    col: 0,
                    len: 3,
                    row: 1,
                },
            ],
        )?;

        let vertical_cell = rows
            .first()
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected vertical border cell"))?;
        let horizontal_cell = rows
            .get(1)
            .and_then(|row| row.first())
            .ok_or_else(|| report!("expected horizontal border cell"))?;
        let junction_cell = rows
            .get(1)
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected junction border cell"))?;

        pretty_assertions::assert_eq!(vertical_cell.text, "│");
        pretty_assertions::assert_eq!(horizontal_cell.text, "─");
        pretty_assertions::assert_eq!(junction_cell.text, "┼");
        pretty_assertions::assert_eq!(vertical_cell.style, self::border_style());
        Ok(())
    }

    #[test]
    fn test_session_layout_close_when_nested_pane_closes_collapses_parent_split() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(self::metadata("sh", 3), PaneSplitAxis::Horizontal)?;
        let close = layout.close_active_pane(3)?;

        assert2::assert!(!close.final_pane);
        pretty_assertions::assert_eq!(close.pane_id.as_ref(), "pane-3");
        pretty_assertions::assert_eq!(layout.active_tab()?.active_pane.as_ref(), "pane-2");
        pretty_assertions::assert_eq!(self::layout_active_tab_pane_ids(&layout)?, vec!["pane-1", "pane-2"]);
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 24),
            ],
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical_left(
        PaneSplitAxis::Vertical,
        PaneResizeDirection::Left,
        vec![
            ("pane-1", 0, 0, 36, 24),
            ("pane-2", 37, 0, 43, 24),
        ],
    )]
    #[case::vertical_right(
        PaneSplitAxis::Vertical,
        PaneResizeDirection::Right,
        vec![
            ("pane-1", 0, 0, 43, 24),
            ("pane-2", 44, 0, 36, 24),
        ],
    )]
    #[case::horizontal_up(
        PaneSplitAxis::Horizontal,
        PaneResizeDirection::Up,
        vec![
            ("pane-1", 0, 0, 80, 10),
            ("pane-2", 0, 11, 80, 13),
        ],
    )]
    #[case::horizontal_down(
        PaneSplitAxis::Horizontal,
        PaneResizeDirection::Down,
        vec![
            ("pane-1", 0, 0, 80, 13),
            ("pane-2", 0, 14, 80, 10),
        ],
    )]
    fn test_session_layout_resize_active_pane_when_resize_command_arrives_updates_geometry(
        #[case] split_axis: PaneSplitAxis,
        #[case] direction: PaneResizeDirection,
        #[case] expected_regions: Vec<(&str, u16, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), split_axis)?;

        assert2::assert!(layout.resize_active_pane(direction)?);
        let expected_regions = expected_regions
            .into_iter()
            .map(|(id, col, row, cols, rows)| (id.to_owned(), col, row, cols, rows))
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            expected_regions
        );
        Ok(())
    }

    #[test]
    fn test_session_layout_resize_nested_splits_resizes_nearest_matching_axis() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(self::metadata("sh", 3), PaneSplitAxis::Horizontal)?;

        assert2::assert!(layout.resize_active_pane(PaneResizeDirection::Up)?);
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 10),
                ("pane-3".to_owned(), 41, 11, 39, 13),
            ],
        );

        assert2::assert!(layout.resize_active_pane(PaneResizeDirection::Left)?);
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 36, 24),
                ("pane-2".to_owned(), 37, 0, 43, 10),
                ("pane-3".to_owned(), 37, 11, 43, 13),
            ],
        );
        Ok(())
    }

    #[test]
    fn test_session_layout_metadata_when_nested_panes_exist_round_trips_tree() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        fs::create_dir_all(&config.paths.root)?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(self::metadata("sh", 3), PaneSplitAxis::Horizontal)?;
        self::write_layout_metadata(&config.paths, &layout)?;

        let loaded = self::load_layout_metadata(&config.paths, &config)?
            .ok_or_else(|| report!("expected muxr layout metadata to load"))?;

        pretty_assertions::assert_eq!(loaded.active_tab()?.active_pane.as_ref(), "pane-3");
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&loaded, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 12),
                ("pane-3".to_owned(), 41, 13, 39, 11),
            ],
        );
        Ok(())
    }

    #[test]
    fn test_session_layout_metadata_when_resized_panes_exist_round_trips_split_ratio() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        fs::create_dir_all(&config.paths.root)?;
        let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        assert2::assert!(layout.resize_active_pane(PaneResizeDirection::Left)?);
        self::write_layout_metadata(&config.paths, &layout)?;

        let loaded = self::load_layout_metadata(&config.paths, &config)?
            .ok_or_else(|| report!("expected muxr layout metadata to load"))?;

        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&loaded, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 36, 24),
                ("pane-2".to_owned(), 37, 0, 43, 24),
            ],
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::create_tab(
        ClientKeyCode::Char('E'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bE",
        ClientCommand::CreateTab
    )]
    #[case::focus_previous_tab(
        ClientKeyCode::Char('P'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bP",
        ClientCommand::FocusPreviousTab
    )]
    #[case::focus_next_tab(
        ClientKeyCode::Char('N'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bN",
        ClientCommand::FocusNextTab
    )]
    #[case::move_tab_previous(
        ClientKeyCode::Char('p'),
        ClientKeyModifiers::CTRL_ALT,
        b"\x1b\x10",
        ClientCommand::MoveTabPrevious
    )]
    #[case::move_tab_next(
        ClientKeyCode::Char('n'),
        ClientKeyModifiers::CTRL_ALT,
        b"\x1b\x0e",
        ClientCommand::MoveTabNext
    )]
    #[case::focus_pane_left(
        ClientKeyCode::Char('H'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bH",
        ClientCommand::FocusPane(PaneFocusDirection::Left)
    )]
    #[case::focus_pane_down(
        ClientKeyCode::Char('J'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bJ",
        ClientCommand::FocusPane(PaneFocusDirection::Down)
    )]
    #[case::focus_pane_up(
        ClientKeyCode::Char('K'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bK",
        ClientCommand::FocusPane(PaneFocusDirection::Up)
    )]
    #[case::focus_pane_right(
        ClientKeyCode::Char('L'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bL",
        ClientCommand::FocusPane(PaneFocusDirection::Right)
    )]
    #[case::split_pane_vertical(
        ClientKeyCode::Char('V'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bV",
        ClientCommand::SplitPaneVertical
    )]
    #[case::split_pane_horizontal(
        ClientKeyCode::Char('D'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bD",
        ClientCommand::SplitPaneHorizontal
    )]
    #[case::close_pane(
        ClientKeyCode::Char('W'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bW",
        ClientCommand::ClosePane
    )]
    fn test_resolve_key_when_normal_bound_key_arrives_returns_command(
        #[case] code: ClientKeyCode,
        #[case] modifiers: ClientKeyModifiers,
        #[case] raw_bytes: &[u8],
        #[case] command: ClientCommand,
    ) {
        let mut input_mode = ServerInputMode::Normal;
        let key = ClientKey::new(code, modifiers, raw_bytes.to_vec());

        pretty_assertions::assert_eq!(resolve_key(&mut input_mode, &key), KeyResolution::Command(command),);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    #[test]
    fn test_resolve_key_when_unbound_key_arrives_returns_raw() {
        let mut input_mode = ServerInputMode::Normal;
        let key = ClientKey::new(ClientKeyCode::Char('x'), ClientKeyModifiers::NONE, b"x".to_vec());

        pretty_assertions::assert_eq!(resolve_key(&mut input_mode, &key), KeyResolution::Raw);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    #[rstest::rstest]
    #[case::left(ClientKeyCode::Char('h'), ClientCommand::ResizePane(PaneResizeDirection::Left))]
    #[case::down(ClientKeyCode::Char('j'), ClientCommand::ResizePane(PaneResizeDirection::Down))]
    #[case::up(ClientKeyCode::Char('k'), ClientCommand::ResizePane(PaneResizeDirection::Up))]
    #[case::right(ClientKeyCode::Char('l'), ClientCommand::ResizePane(PaneResizeDirection::Right))]
    #[case::arrow_left(ClientKeyCode::Left, ClientCommand::ResizePane(PaneResizeDirection::Left))]
    #[case::arrow_down(ClientKeyCode::Down, ClientCommand::ResizePane(PaneResizeDirection::Down))]
    #[case::arrow_up(ClientKeyCode::Up, ClientCommand::ResizePane(PaneResizeDirection::Up))]
    #[case::arrow_right(ClientKeyCode::Right, ClientCommand::ResizePane(PaneResizeDirection::Right))]
    fn test_resolve_key_when_resize_mode_key_arrives_returns_resize_command(
        #[case] code: ClientKeyCode,
        #[case] command: ClientCommand,
    ) {
        let mut input_mode = ServerInputMode::Resize;
        let key = ClientKey::new(code, ClientKeyModifiers::NONE, b"x".to_vec());

        pretty_assertions::assert_eq!(resolve_key(&mut input_mode, &key), KeyResolution::Command(command),);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Resize);
    }

    #[test]
    fn test_resolve_key_when_resize_mode_enter_and_exit_arrive_updates_server_mode() {
        let mut input_mode = ServerInputMode::Normal;
        let enter = ClientKey::new(
            ClientKeyCode::Char('R'),
            ClientKeyModifiers::SHIFT_ALT,
            b"\x1bR".to_vec(),
        );
        let exit = ClientKey::new(ClientKeyCode::Esc, ClientKeyModifiers::NONE, b"\x1b".to_vec());

        pretty_assertions::assert_eq!(
            resolve_key(&mut input_mode, &enter),
            KeyResolution::Command(ClientCommand::EnterResizeMode),
        );
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Resize);
        pretty_assertions::assert_eq!(
            resolve_key(&mut input_mode, &exit),
            KeyResolution::Command(ClientCommand::ExitMode),
        );
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    #[test]
    fn test_serve_when_key_request_arrives_writes_raw_bytes_and_stays_attached() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), ShellCommand::new("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('x'),
                    ClientKeyModifiers::NONE,
                    b"x\n".to_vec(),
                )))
                .await?;

            self::read_until_output_contains(&mut client, b"x").await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_create_tab_key_arrives_sends_layout_and_persists_metadata() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), ShellCommand::new("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('E'),
                    ClientKeyModifiers::SHIFT_ALT,
                    b"\x1bE".to_vec(),
                )))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            pretty_assertions::assert_eq!(layout.active_tab.as_ref(), "tab-2");
            pretty_assertions::assert_eq!(
                layout.tabs.iter().map(|tab| tab.id.as_ref()).collect::<Vec<_>>(),
                vec!["tab-1", "tab-2"],
            );
            self::assert_layout_metadata_tabs(&paths, &["tab-1", "tab-2"], "tab-2")?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_layout_metadata_exists_restores_tab_order_on_attach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let config = self::server_config(tempdir.path(), "work")?;
            fs::create_dir_all(&config.paths.root)?;
            let mut layout = SessionLayout::initial(&config, self::metadata("sh", 1))?;
            layout.create_tab(self::metadata("sh", 2))?;
            self::write_layout_metadata(&config.paths, &layout)?;
            let paths = config.paths.clone();
            let session = config.session.clone();
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), ShellCommand::new("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let client = self::open_attached_client(&session, &paths).await?;
            pretty_assertions::assert_eq!(client.layout.active_tab.as_ref(), "tab-2");
            pretty_assertions::assert_eq!(
                client.layout.tabs.iter().map(|tab| tab.id.as_ref()).collect::<Vec<_>>(),
                vec!["tab-1", "tab-2"],
            );
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_split_pane_key_arrives_sends_layout_and_routes_input_to_new_pane() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), ShellCommand::new("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('V'),
                    ClientKeyModifiers::SHIFT_ALT,
                    b"\x1bV".to_vec(),
                )))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout.tabs.first().ok_or_else(|| report!("expected tab after split"))?;
            pretty_assertions::assert_eq!(tab.active_pane.as_ref(), "pane-2");
            pretty_assertions::assert_eq!(
                tab.panes.iter().map(|pane| pane.id.as_ref()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );

            client
                .writer
                .send_request(&ClientRequest::Input(b"new-pane\n".to_vec()))
                .await?;
            self::read_until_output_contains(&mut client, b"new-pane").await?;
            self::assert_layout_metadata_panes(&paths, &["pane-1", "pane-2"], "pane-2")?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_close_pane_key_arrives_removes_active_pane_and_keeps_remaining_pty() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), ShellCommand::new("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('V'),
                    ClientKeyModifiers::SHIFT_ALT,
                    b"\x1bV".to_vec(),
                )))
                .await?;
            drop(self::read_until_layout(&mut client).await?);

            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('W'),
                    ClientKeyModifiers::SHIFT_ALT,
                    b"\x1bW".to_vec(),
                )))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout.tabs.first().ok_or_else(|| report!("expected tab after close"))?;
            pretty_assertions::assert_eq!(tab.active_pane.as_ref(), "pane-1");
            pretty_assertions::assert_eq!(
                tab.panes.iter().map(|pane| pane.id.as_ref()).collect::<Vec<_>>(),
                vec!["pane-1"],
            );

            client
                .writer
                .send_request(&ClientRequest::Input(b"remaining\n".to_vec()))
                .await?;
            self::read_until_output_contains(&mut client, b"remaining").await?;
            self::assert_layout_metadata_panes(&paths, &["pane-1"], "pane-1")?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_final_pane_is_closed_persists_and_exits() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), ShellCommand::new("/bin/cat"));

        self::runtime()?.block_on(async {
            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('W'),
                    ClientKeyModifiers::SHIFT_ALT,
                    b"\x1bW".to_vec(),
                )))
                .await?;
            self::read_client_until_detached(&mut client).await?;
            drop(client);
            Ok::<(), rootcause::Report>(())
        })?;

        self::join_server_with_timeout(handle)?;
        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        self::assert_final_closed_layout_metadata(&paths)?;
        Ok(())
    }

    #[test]
    fn test_serve_resize_mode_sequence_resizes_and_escape_exits_mode() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), ShellCommand::new("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('V'),
                    ClientKeyModifiers::SHIFT_ALT,
                    b"\x1bV".to_vec(),
                )))
                .await?;
            drop(self::read_until_layout(&mut client).await?);
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('R'),
                    ClientKeyModifiers::SHIFT_ALT,
                    b"\x1bR".to_vec(),
                )))
                .await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('h'),
                    ClientKeyModifiers::NONE,
                    b"h".to_vec(),
                )))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs
                .first()
                .ok_or_else(|| report!("expected tab after resize"))?;
            pretty_assertions::assert_eq!(tab.active_pane.as_ref(), "pane-2");
            pretty_assertions::assert_eq!(
                tab.panes.iter().map(|pane| pane.id.as_ref()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );
            let persisted = self::load_layout_metadata(&paths, &self::server_config(tempdir.path(), "work")?)?
                .ok_or_else(|| report!("expected muxr layout metadata to load"))?;
            pretty_assertions::assert_eq!(
                self::layout_active_tab_pane_regions(&persisted, &TerminalSize::new(80, 24)?)?,
                vec![
                    ("pane-1".to_owned(), 0, 0, 36, 24),
                    ("pane-2".to_owned(), 37, 0, 43, 24),
                ],
            );
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Esc,
                    ClientKeyModifiers::NONE,
                    b"\x1b".to_vec(),
                )))
                .await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey::new(
                    ClientKeyCode::Char('x'),
                    ClientKeyModifiers::NONE,
                    b"x\n".to_vec(),
                )))
                .await?;
            self::read_until_output_contains(&mut client, b"x").await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_shell_outputs_while_detached_replays_output_on_reattach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(2),
                ShellCommand::new("/bin/sh")
                    .arg("-c")
                    .arg("printf first; sleep 1; printf second; sleep 30"),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut first_client = self::open_attached_client(&session, &paths).await?;
            self::read_until_output_contains(&mut first_client, b"first").await?;
            self::detach_client(first_client).await?;

            tokio::time::sleep(Duration::from_millis(1200)).await;

            let mut second_client = self::open_attached_client(&session, &paths).await?;
            self::read_until_output_contains(&mut second_client, b"second").await?;
            self::detach_client(second_client).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_floods_input_still_sends_output() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                ShellCommand::new("/bin/sh")
                    .arg("-c")
                    .arg("sleep 0.1; printf ready; sleep 30"),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::hello_request(&session)?).await?;
            let Some(ServerEvent::Hello(_)) = connection.recv_event().await? else {
                return Err(report!("expected server hello"));
            };
            let (mut reader, mut writer) = connection.split();
            let flood_handle = tokio::spawn(async move {
                loop {
                    if writer.send_request(&ClientRequest::Input(Vec::new())).await.is_err() {
                        break;
                    }
                }
            });

            let read_result = self::read_reader_until_output_contains(&mut reader, b"ready").await;
            drop(reader);
            flood_handle.abort();
            drop(flood_handle.await);
            let join_result = self::join_server_with_timeout(handle);

            read_result?;
            join_result
        })
    }

    #[test]
    fn test_serve_when_shell_floods_output_still_detaches() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                ShellCommand::new("/bin/sh").arg("-c").arg("while :; do printf x; done"),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::hello_request(&session)?).await?;
            let Some(ServerEvent::Hello(_)) = connection.recv_event().await? else {
                return Err(report!("expected server hello"));
            };
            self::read_connection_until_output(&mut connection).await?;
            connection.send_request(&ClientRequest::Detach).await?;
            self::read_connection_until_detached(&mut connection).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_shell_exits_removes_socket_and_pid() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(
            &session,
            &paths,
            None,
            ShellCommand::new("/bin/sh").arg("-c").arg("printf done"),
        );

        self::wait_for_socket(&paths.socket)?;
        self::join_server_with_timeout(handle)?;

        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        self::assert_final_layout_metadata(&paths, 0, true)?;
        Ok(())
    }

    #[test]
    fn test_serve_when_shell_exits_with_error_persists_exit_status() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(
            &session,
            &paths,
            None,
            ShellCommand::new("/bin/sh").arg("-c").arg("exit 7"),
        );

        self::wait_for_socket(&paths.socket)?;
        self::join_server_with_timeout(handle)?;

        self::assert_final_layout_metadata(&paths, 7, false)?;
        Ok(())
    }

    #[test]
    fn test_serve_when_startup_fails_after_bind_removes_socket_and_pid() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;

        let result = serve(&ServerConfig {
            session,
            paths: paths.clone(),
            max_accepted_connections: None,
            shell_command: ShellCommand::new("/bin/muxr-missing-shell"),
        });

        assert2::assert!(result.is_err());
        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        Ok(())
    }

    fn spawn_test_server(
        session: &SessionName,
        paths: &SessionPaths,
        max_accepted_connections: usize,
    ) -> thread::JoinHandle<rootcause::Result<()>> {
        self::spawn_test_server_with_shell(
            session,
            paths,
            Some(max_accepted_connections),
            ShellCommand::new("/bin/sh").arg("-c").arg("sleep 30"),
        )
    }

    fn spawn_test_server_with_shell(
        session: &SessionName,
        paths: &SessionPaths,
        max_accepted_connections: Option<usize>,
        shell_command: ShellCommand,
    ) -> thread::JoinHandle<rootcause::Result<()>> {
        thread::spawn({
            let session = session.clone();
            let paths = paths.clone();
            move || {
                serve(&ServerConfig {
                    session,
                    paths,
                    max_accepted_connections,
                    shell_command,
                })
            }
        })
    }

    async fn connect_client(paths: &SessionPaths) -> rootcause::Result<ClientConnection> {
        let started_at = Instant::now();

        loop {
            match ClientConnection::connect(&paths.socket).await {
                Ok(connection) => return Ok(connection),
                Err(error) => {
                    if started_at.elapsed() > SERVER_READY_TIMEOUT {
                        return Err(error);
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn open_attached_client(
        session: &SessionName,
        paths: &SessionPaths,
    ) -> rootcause::Result<AttachedTestClient> {
        let mut connection = self::connect_client(paths).await?;

        connection.send_request(&self::hello_request(session)?).await?;
        let event = connection.recv_event().await?;
        let Some(ServerEvent::Hello(hello)) = event else {
            return Err(report!("expected server hello").attach(format!("{event:?}")));
        };
        let server_pid = hello.server_pid;
        let layout = hello.layout;
        let (reader, writer) = connection.split();

        Ok(AttachedTestClient {
            layout,
            reader,
            server_pid,
            writer,
        })
    }

    async fn attach_and_detach(session: &SessionName, paths: &SessionPaths) -> rootcause::Result<ServerPid> {
        let client = self::open_attached_client(session, paths).await?;
        let pid = client.server_pid;

        self::detach_client(client).await?;
        Ok(pid)
    }

    async fn detach_client(mut client: AttachedTestClient) -> rootcause::Result<()> {
        client.writer.send_request(&ClientRequest::Detach).await?;
        self::read_client_until_detached(&mut client).await
    }

    async fn read_client_until_detached(client: &mut AttachedTestClient) -> rootcause::Result<()> {
        loop {
            match client.reader.recv_event().await? {
                Some(ServerEvent::Detached) => break,
                Some(ServerEvent::Ping) => client.writer.send_request(&ClientRequest::Pong).await?,
                Some(
                    ServerEvent::Hello(_)
                    | ServerEvent::Pong
                    | ServerEvent::Layout(_)
                    | ServerEvent::Render(_)
                    | ServerEvent::Output(_),
                ) => {}
                Some(event) => return Err(report!("expected detached event").attach(format!("{event:?}"))),
                None => return Err(report!("expected detached event")),
            }
        }
        Ok(())
    }

    fn join_server(handle: thread::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
        handle
            .join()
            .unwrap_or_else(|_| Err(report!("test muxr server thread panicked")))
    }

    fn join_server_with_timeout(handle: thread::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
        let started_at = Instant::now();
        while !handle.is_finished() {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr test server exit"));
            }

            thread::sleep(Duration::from_millis(10));
        }

        self::join_server(handle)
    }

    fn session_paths(base: &Path, raw: &str) -> rootcause::Result<(SessionName, SessionPaths)> {
        let session = raw.parse()?;
        let state_root = base.join("muxr");
        let root = state_root.join("sessions").join(raw);

        Ok((
            session,
            SessionPaths {
                socket: state_root.join("s").join(format!("{raw}.sock")),
                pid: root.join("server.pid"),
                layout: root.join("layout.json"),
                panes: root.join("panes"),
                root,
            },
        ))
    }

    fn server_config(base: &Path, raw: &str) -> rootcause::Result<ServerConfig> {
        let (session, paths) = self::session_paths(base, raw)?;
        Ok(ServerConfig {
            session,
            paths,
            max_accepted_connections: None,
            shell_command: ShellCommand::new("/bin/sh"),
        })
    }

    fn metadata(command_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            command_label: command_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }

    fn layout_tab_ids(layout: &SessionLayout) -> Vec<&str> {
        layout.tabs.iter().map(|tab| tab.id.as_ref()).collect()
    }

    fn layout_active_tab_pane_ids(layout: &SessionLayout) -> rootcause::Result<Vec<&str>> {
        Ok(layout.active_tab()?.pane_ids())
    }

    fn layout_active_tab_pane_regions(
        layout: &SessionLayout,
        size: &TerminalSize,
    ) -> rootcause::Result<Vec<PaneRegionSnapshot>> {
        Ok(layout
            .pane_regions(size)?
            .iter()
            .map(|region| {
                (
                    region.id.as_ref().to_owned(),
                    region.col,
                    region.row,
                    region.cols,
                    region.rows,
                )
            })
            .collect())
    }

    fn layout_active_tab_pane_borders(
        layout: &SessionLayout,
        size: &TerminalSize,
    ) -> rootcause::Result<Vec<(PaneBorderAxis, u16, u16, u16)>> {
        Ok(layout
            .pane_layout(size)?
            .borders
            .iter()
            .map(|border| (border.axis, border.col, border.row, border.len))
            .collect())
    }

    fn make_public_session_dirs(paths: &SessionPaths) -> rootcause::Result<()> {
        for path in self::session_private_dirs(paths)? {
            fs::create_dir_all(path).context("failed to create public muxr test dir")?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))
                .context("failed to set public muxr test dir permissions")?;
        }
        Ok(())
    }

    fn assert_session_paths_are_private(paths: &SessionPaths) -> rootcause::Result<()> {
        for path in self::session_private_dirs(paths)? {
            self::assert_mode(path, PRIVATE_DIR_MODE)?;
        }
        self::assert_mode(&paths.socket, PRIVATE_SOCKET_MODE)?;
        Ok(())
    }

    fn session_private_dirs(paths: &SessionPaths) -> rootcause::Result<Vec<&Path>> {
        let socket_root = self::parent_path(&paths.socket, "socket root")?;
        let state_root = self::parent_path(socket_root, "state root")?;
        let sessions_root = self::parent_path(&paths.root, "sessions root")?;

        Ok(vec![
            state_root,
            sessions_root,
            socket_root,
            paths.root.as_path(),
            paths.panes.as_path(),
        ])
    }

    fn parent_path<'a>(path: &'a Path, label: &str) -> rootcause::Result<&'a Path> {
        path.parent()
            .ok_or_else(|| report!("muxr test path has no parent").attach(format!("label={label}")))
    }

    fn assert_mode(path: &Path, expected_mode: u32) -> rootcause::Result<()> {
        let mode = fs::metadata(path)
            .context("failed to inspect muxr test path mode")?
            .permissions()
            .mode()
            & 0o777;

        pretty_assertions::assert_eq!(mode, expected_mode);
        Ok(())
    }

    fn wait_for_socket(path: &Path) -> rootcause::Result<()> {
        self::wait_for_path(path)
    }

    fn wait_for_path(path: &Path) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            if path.exists() {
                return Ok(());
            }

            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr test path").attach(path.display().to_string()));
            }

            thread::sleep(Duration::from_millis(10));
        }
    }

    fn hello_request(session: &SessionName) -> rootcause::Result<ClientRequest> {
        Ok(ClientRequest::Hello(ClientHello {
            protocol_version: PROTOCOL_VERSION,
            session: session.clone(),
            terminal_size: self::terminal_size()?,
        }))
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(80, 24)
    }

    async fn read_until_layout(client: &mut AttachedTestClient) -> rootcause::Result<LayoutSnapshot> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr layout update"));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Layout(layout) => return Ok(layout),
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Hello(_)
                | ServerEvent::Pong
                | ServerEvent::Render(_)
                | ServerEvent::Output(_)
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_output_contains(client: &mut AttachedTestClient, needle: &[u8]) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut output = Vec::new();
        let mut rendered = String::new();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr pty output")
                    .attach(String::from_utf8_lossy(&output).into_owned())
                    .attach(rendered));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Output(bytes) => {
                    output.extend_from_slice(&bytes);
                    if self::contains_bytes(&output, needle) {
                        return Ok(());
                    }
                }
                ServerEvent::Render(update) => {
                    rendered.push_str(&self::render_update_text(&update));
                    if self::render_update_contains(&update, needle) {
                        return Ok(());
                    }
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Hello(_) | ServerEvent::Pong | ServerEvent::Layout(_) | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_reader_until_output_contains(reader: &mut ClientEventReader, needle: &[u8]) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut output = Vec::new();
        let mut rendered = String::new();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr pty output")
                    .attach(String::from_utf8_lossy(&output).into_owned())
                    .attach(rendered));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Output(bytes) => {
                    output.extend_from_slice(&bytes);
                    if self::contains_bytes(&output, needle) {
                        return Ok(());
                    }
                }
                ServerEvent::Render(update) => {
                    rendered.push_str(&self::render_update_text(&update));
                    if self::render_update_contains(&update, needle) {
                        return Ok(());
                    }
                }
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Hello(_)
                | ServerEvent::Ping
                | ServerEvent::Pong
                | ServerEvent::Layout(_)
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_connection_until_output(connection: &mut ClientConnection) -> rootcause::Result<()> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr output event"));
            }

            match tokio::time::timeout(Duration::from_millis(50), connection.recv_event()).await {
                Ok(Ok(Some(ServerEvent::Output(_)))) => return Ok(()),
                Ok(Ok(Some(ServerEvent::Render(update)))) => {
                    if self::render_update_contains(&update, b"x") {
                        return Ok(());
                    }
                }
                Ok(Ok(Some(ServerEvent::Ping))) => connection.send_request(&ClientRequest::Pong).await?,
                Ok(Ok(Some(ServerEvent::Error(error)))) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                Ok(Ok(
                    Some(ServerEvent::Hello(_) | ServerEvent::Pong | ServerEvent::Layout(_) | ServerEvent::Detached)
                    | None,
                ))
                | Err(_) => {}
                Ok(Err(error)) => return Err(error),
            }
        }
    }

    async fn read_connection_until_detached(connection: &mut ClientConnection) -> rootcause::Result<()> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr detach ack"));
            }

            match tokio::time::timeout(Duration::from_millis(50), connection.recv_event()).await {
                Ok(Ok(Some(ServerEvent::Detached))) => return Ok(()),
                Ok(Ok(Some(ServerEvent::Ping))) => connection.send_request(&ClientRequest::Pong).await?,
                Ok(Ok(Some(ServerEvent::Error(error)))) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                Ok(Ok(
                    Some(
                        ServerEvent::Hello(_)
                        | ServerEvent::Pong
                        | ServerEvent::Layout(_)
                        | ServerEvent::Render(_)
                        | ServerEvent::Output(_),
                    )
                    | None,
                ))
                | Err(_) => {}
                Ok(Err(error)) => return Err(error),
            }
        }
    }

    fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|window| window == needle)
    }

    fn render_update_contains(update: &RenderUpdate, needle: &[u8]) -> bool {
        let needle = String::from_utf8_lossy(needle);
        self::render_update_text(update).contains(needle.as_ref())
    }

    fn render_update_text(update: &RenderUpdate) -> String {
        match update {
            RenderUpdate::Baseline(baseline) => self::render_rows_text(&baseline.rows),
            RenderUpdate::Diff(diff) => self::render_rows_text(&diff.rows),
        }
    }

    fn render_rows_text(rows: &[RenderRowSpan]) -> String {
        rows.iter()
            .map(|row| row.cells.iter().map(|cell| cell.text.as_str()).collect::<String>())
            .collect()
    }

    fn assert_final_layout_metadata(
        paths: &SessionPaths,
        expected_code: u64,
        expected_success: bool,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let pane = &layout["tabs"][0]["pane_tree"]["pane"];

        pretty_assertions::assert_eq!(layout["version"].as_u64(), Some(u64::from(LAYOUT_VERSION)));
        pretty_assertions::assert_eq!(layout["session"].as_str(), Some("work"));
        pretty_assertions::assert_eq!(layout["active_tab"].as_str(), Some("tab-1"));
        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_str(), Some("pane-1"));
        pretty_assertions::assert_eq!(pane["id"].as_str(), Some("pane-1"));
        pretty_assertions::assert_eq!(pane["command_label"].as_str(), Some("sh"));
        assert2::assert!(pane["started_at"].as_u64().is_some());
        assert2::assert!(pane["exited_at"].as_u64().is_some());
        pretty_assertions::assert_eq!(pane["exit_status"]["code"].as_u64(), Some(expected_code));
        pretty_assertions::assert_eq!(pane["exit_status"]["success"].as_bool(), Some(expected_success));
        Ok(())
    }

    fn assert_layout_metadata_tabs(
        paths: &SessionPaths,
        expected_tabs: &[&str],
        expected_active: &str,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let Some(tabs) = layout["tabs"].as_array() else {
            return Err(report!("muxr test layout metadata tabs are missing"));
        };
        let actual_tabs = tabs
            .iter()
            .map(|tab| {
                tab["id"]
                    .as_str()
                    .ok_or_else(|| report!("muxr test layout metadata tab id is missing"))
            })
            .collect::<rootcause::Result<Vec<_>>>()?;

        pretty_assertions::assert_eq!(layout["active_tab"].as_str(), Some(expected_active));
        pretty_assertions::assert_eq!(actual_tabs, expected_tabs.to_vec());
        Ok(())
    }

    fn assert_layout_metadata_panes(
        paths: &SessionPaths,
        expected_panes: &[&str],
        expected_active: &str,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let actual_panes = self::json_pane_tree_leaf_ids(&layout["tabs"][0]["pane_tree"])?;

        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_str(), Some(expected_active));
        pretty_assertions::assert_eq!(actual_panes, expected_panes.to_vec());
        Ok(())
    }

    fn assert_final_closed_layout_metadata(paths: &SessionPaths) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let pane = &layout["tabs"][0]["pane_tree"]["pane"];

        pretty_assertions::assert_eq!(layout["active_tab"].as_str(), Some("tab-1"));
        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_str(), Some("pane-1"));
        pretty_assertions::assert_eq!(pane["id"].as_str(), Some("pane-1"));
        assert2::assert!(pane["exited_at"].as_u64().is_some());
        assert2::assert!(pane["exit_status"].is_null());
        Ok(())
    }

    fn json_pane_tree_leaf_ids(node: &serde_json::Value) -> rootcause::Result<Vec<&str>> {
        let mut ids = Vec::new();
        self::collect_json_pane_tree_leaf_ids(node, &mut ids)?;
        Ok(ids)
    }

    fn collect_json_pane_tree_leaf_ids<'a>(
        node: &'a serde_json::Value,
        ids: &mut Vec<&'a str>,
    ) -> rootcause::Result<()> {
        match node["kind"].as_str() {
            Some("leaf") => {
                let Some(id) = node["pane"]["id"].as_str() else {
                    return Err(report!("muxr test layout metadata pane id is missing"));
                };
                ids.push(id);
                Ok(())
            }
            Some("split") => {
                self::collect_json_pane_tree_leaf_ids(&node["first"], ids)?;
                self::collect_json_pane_tree_leaf_ids(&node["second"], ids)
            }
            Some(kind) => {
                Err(report!("muxr test layout metadata pane tree kind is invalid").attach(format!("kind={kind}")))
            }
            None => Err(report!("muxr test layout metadata pane tree kind is missing")),
        }
    }

    fn runtime() -> rootcause::Result<tokio::runtime::Runtime> {
        Ok(tokio::runtime::Runtime::new().context("failed to build muxr server test runtime")?)
    }
}
