//! Private muxr server runner.
//!
//! The public `muxr` binary remains the only user entrypoint. This separate process owns PTYs, session state, and
//! scrollback so the long-lived runtime does not inherit picker/UI-only CLI dependencies and memory attribution stays
//! clear while debugging server footprint.

use muxr_core::ServerRunnerArgs;

fn main() {
    if let Err(error) = self::run() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}

fn run() -> rootcause::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let ServerRunnerArgs {
        external_layout,
        session,
    } = ServerRunnerArgs::parse(&args)?;
    muxr_server::serve_session(&session, external_layout)
}
