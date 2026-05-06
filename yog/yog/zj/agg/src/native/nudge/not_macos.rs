use std::path::Path;

use notify_rust::Notification;
use rootcause::prelude::ResultExt;

use super::NudgeInput;

pub fn run(input: NudgeInput<'_>) -> rootcause::Result<()> {
    let mut notification = Notification::new();
    notification.summary(input.summary).body(input.body);
    if let Some(image_path) = input.image_path.filter(|image_path| Path::new(image_path).is_file()) {
        notification.image_path(image_path);
    }
    notification
        .show()
        .context("failed to send desktop notification")
        .attach_with(|| format!("summary={:?}", input.summary))
        .attach_with(|| format!("body={:?}", input.body))
        .attach_with(|| format!("tab_id={:?}", input.tab_id))
        .attach_with(|| format!("pane_id={:?}", input.pane_id))
        .attach_with(|| format!("zj_session={:?}", input.zj_session))
        .attach_with(|| format!("image_path={:?}", input.image_path))?;

    Ok(())
}
