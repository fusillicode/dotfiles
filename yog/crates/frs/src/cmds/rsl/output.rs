//! Reports changes that could not be applied safely.

#[derive(Default)]
pub struct RslOutput {
    skips: Vec<String>,
}

impl RslOutput {
    pub(super) const fn new(skips: Vec<String>) -> Self {
        Self { skips }
    }

    pub fn render(&self) -> String {
        self.skips.join("\n")
    }

    pub const fn is_empty(&self) -> bool {
        self.skips.is_empty()
    }
}
