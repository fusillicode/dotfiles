use std::path::Path;

use mac_notification_sys::NotificationResponse;
use rootcause::prelude::ResultExt;

use super::RunInput;

pub fn run(input: RunInput<'_>) -> rootcause::Result<()> {
    let session = std::env::var("ZELLIJ_SESSION_NAME").ok();
    mac_notification_sys::set_application("org.alacritty").context("failed to set notification application")?;

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
        .attach_with(|| format!("image_path={:?}", input.image_path))?;

    match response {
        NotificationResponse::Click | NotificationResponse::ActionButton(_) => {
            let _ = ytil_zellij::focus_tab_terminal_pane(session.as_deref(), input.tab_id, input.pane_id);
        }
        NotificationResponse::None | NotificationResponse::CloseButton(_) | NotificationResponse::Reply(_) => {}
    }

    Ok(())
}
