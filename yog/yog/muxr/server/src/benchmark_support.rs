//! Feature-gated server performance seams for muxr development.

/// Observe the current benchmark process through the production OS lookup path.
#[must_use]
pub fn observe_current_process() -> (bool, bool) {
    crate::pane::cmd::benchmark_current_process_observation()
}
