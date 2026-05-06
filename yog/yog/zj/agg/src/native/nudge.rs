use chrono::Local;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod not_macos;

#[derive(Clone, Copy)]
pub struct RunInput<'a> {
    pub summary: &'a str,
    pub body: &'a str,
    pub tab_id: usize,
    pub pane_id: u32,
    pub image_path: Option<&'a str>,
}

pub fn run(input: RunInput<'_>) -> rootcause::Result<()> {
    let summary = format!("{} @ {}", input.summary, Local::now().format("%H:%M:%S"));
    let input = RunInput {
        summary: &summary,
        ..input
    };

    #[cfg(target_os = "macos")]
    {
        macos::run(input)
    }

    #[cfg(not(target_os = "macos"))]
    {
        not_macos::run(input)
    }
}
