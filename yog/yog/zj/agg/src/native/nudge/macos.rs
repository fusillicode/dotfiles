use std::path::Path;

use mac_notification_sys::NotificationResponse;
use rootcause::prelude::ResultExt;
use ytil_cmd::CmdExt;

use super::NudgeInput;

const TERMINAL_BUNDLE_ID: &str = "org.alacritty";
const TERMINAL_ACTIVATE_SCRIPT: &str = "tell application id \"org.alacritty\" to activate";

pub fn run(input: NudgeInput<'_>) -> rootcause::Result<()> {
    mac_notification_sys::set_application(TERMINAL_BUNDLE_ID).context("failed to set notification application")?;

    let mut notification = mac_notification_sys::Notification::new();
    notification
        .title(input.summary)
        .message(input.body)
        .wait_for_click(true);
    if let Some(image_path) = input.image_path.filter(|image_path| Path::new(image_path).is_file()) {
        notification.content_image(image_path);
    }

    let response = notification
        .send()
        .context("failed to send desktop notification")
        .attach_with(|| format!("summary={:?}", input.summary))
        .attach_with(|| format!("body={:?}", input.body))
        .attach_with(|| format!("zj_session={:?}", input.zj_session))
        .attach_with(|| format!("image_path={:?}", input.image_path))?;

    match response {
        NotificationResponse::Click | NotificationResponse::ActionButton(_) => {
            let _ = ytil_zellij::focus_tab_terminal_pane(input.zj_session, input.tab_id, input.pane_id);
            let _ = self::activate_terminal_application();
        }
        NotificationResponse::None | NotificationResponse::CloseButton(_) | NotificationResponse::Reply(_) => {}
    }

    Ok(())
}

/// Notification clicks focus Zellij internally, but macOS keeps the current foreground app.
/// Bring Alacritty forward so clicking a nudge from another app visibly returns to the focused pane.
fn activate_terminal_application() -> rootcause::Result<()> {
    ytil_cmd::silent_cmd("osascript")
        .args(["-e", TERMINAL_ACTIVATE_SCRIPT])
        .exec()
        .context("failed to activate terminal application")?;
    Ok(())
}
