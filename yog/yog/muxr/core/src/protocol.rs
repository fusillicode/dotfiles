pub use keyboard_input::ClientKey;
pub use keyboard_input::ClientKeyCode;
pub use keyboard_input::ClientKeyModifiers;
pub use pane_layout::GitStats;
pub use pane_layout::LayoutSnapshot;
pub use pane_layout::PaneId;
pub use pane_layout::PaneMouseMode;
pub use pane_layout::PaneRegionSnapshot;
pub use pane_layout::PaneRegionsSnapshot;
pub use pane_layout::PaneSnapshot;
pub use pane_layout::TabId;
pub use pane_layout::TabSnapshot;
pub use pane_mouse::ClientMouseEvent;
pub use pane_mouse::ClientMouseEventPhase;
pub use pane_mouse::ClientMousePosition;
pub use pane_render::RenderBaseline;
pub use pane_render::RenderCell;
pub use pane_render::RenderCellWidth;
pub use pane_render::RenderColor;
pub use pane_render::RenderCursor;
pub use pane_render::RenderDiff;
pub use pane_render::RenderHyperlink;
pub use pane_render::RenderRowSpan;
pub use pane_render::RenderStyle;
pub use pane_render::RenderTextStyle;
pub use pane_render::RenderUpdate;
pub use pane_scroll::PaneScrollDirection;
pub use session_attach::AttachAccepted;
pub use session_attach::AttachRequest;
pub use terminal::TerminalSize;
pub use tracked_process::TrackedProcessState;
pub use wire::ClientRequest;
pub use wire::ServerError;
pub use wire::ServerEvent;
pub use wire::decode_client_request;
pub use wire::decode_server_event;
pub use wire::encode_client_request;
pub use wire::encode_server_event;

mod keyboard_input;
mod pane_layout;
mod pane_mouse;
mod pane_render;
mod pane_scroll;
mod session_attach;
mod terminal;
mod tracked_process;
mod wire;

fn rkyv_deserialize_error<E>(error: impl std::fmt::Display) -> E
where
    E: rkyv::rancor::Source,
{
    <E as rkyv::rancor::Source>::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}
