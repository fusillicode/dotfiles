#![feature(exit_status_error)]

use anyhow::anyhow;

pub mod github;
pub mod hx;
pub mod system;
pub mod wezterm;

pub fn get_args() -> Vec<String> {
    let mut args = std::env::args();
    args.next();
    args.collect::<Vec<String>>()
}

pub fn split_cmd_and_args(args: &[String]) -> anyhow::Result<(&str, Vec<&str>)> {
    args.split_first()
        .map(|(cmd, cmd_args)| (cmd.as_str(), cmd_args.iter().map(String::as_str).collect()))
        .ok_or_else(|| anyhow!("cannot parse cmd and args from input args {args:?}"))
}

// Needed because calling ebi from wezterm open-uri handler doesn't retain the PATH
pub fn load_additional_paths() -> anyhow::Result<()> {
    let home = std::env::var("HOME")?;

    let new_path = [
        &std::env::var("PATH").unwrap_or_else(|_| String::new()),
        "/opt/homebrew/bin",
        &format!("{home}/.local/bin"),
    ]
    .join(":");

    std::env::set_var("PATH", &new_path);
    Ok(())
}
