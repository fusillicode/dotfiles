#![feature(exit_status_error)]

use anyhow::anyhow;

mod cmds;
mod utils;

fn main() -> anyhow::Result<()> {
    let args = get_args();
    let (cmd, args) = split_cmd_and_args(&args)?;
    load_additional_paths()?;

    match cmd {
        "yhfp" => cmds::yhfp::run(args.into_iter()),
        "yghfl" => cmds::yghfl::run(args.into_iter()),
        "oe" => cmds::oe::run(args.into_iter()),
        "idt" => cmds::idt::run(args.into_iter()),
        "catl" => cmds::catl::run(args.into_iter()),
        unknown_cmd => Err(anyhow!("unknown cmd '{unknown_cmd}' in args {args:?}")),
    }
}

fn get_args() -> Vec<String> {
    let mut args = std::env::args();
    args.next();
    args.collect::<Vec<String>>()
}

fn split_cmd_and_args(args: &[String]) -> anyhow::Result<(&str, Vec<&str>)> {
    args.split_first()
        .map(|(cmd, cmd_args)| (cmd.as_str(), cmd_args.iter().map(String::as_str).collect()))
        .ok_or_else(|| anyhow!("cannot parse cmd and args from input args {args:?}"))
}

// Needed because calling ebi from wezterm open-uri handler doesn't retain the PATH
fn load_additional_paths() -> anyhow::Result<()> {
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
