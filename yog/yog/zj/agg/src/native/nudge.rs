use std::path::Path;

use chrono::Local;
use notify_rust::Notification;
use rootcause::prelude::ResultExt;

pub fn run(summary: &str, body: &str, image_path: Option<&str>) -> rootcause::Result<()> {
    #[cfg(target_os = "macos")]
    notify_rust::set_application("org.alacritty").context("failed to set notification application")?;

    let summary = format!("{summary} {}", Local::now().format("%H:%M:%S"));
    let mut notification = Notification::new();
    notification.summary(&summary).body(body);
    if let Some(image_path) = image_path.filter(|image_path| Path::new(image_path).is_file()) {
        notification.image_path(image_path);
    }
    notification
        .show()
        .context("failed to send desktop notification")
        .attach_with(|| format!("summary={summary:?}"))
        .attach_with(|| format!("body={body:?}"))
        .attach_with(|| format!("image_path={image_path:?}"))?;

    Ok(())
}
