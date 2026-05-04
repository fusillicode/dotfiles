use std::path::Path;

use rootcause::prelude::ResultExt;

pub fn run(name: &str, body: &str, image_path: Option<&str>) -> rootcause::Result<()> {
    #[cfg(target_os = "macos")]
    notify_rust::set_application("org.alacritty").context("failed to set notification application")?;

    let mut notification = notify_rust::Notification::new();
    notification.summary(name).body(body);
    if let Some(image_path) = image_path.filter(|image_path| Path::new(image_path).is_file()) {
        notification.image_path(image_path);
    }
    notification.show().context("failed to send desktop notification")?;

    Ok(())
}
