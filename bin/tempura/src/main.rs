#![feature(exit_status_error)]

use anyhow::anyhow;
use anyhow::bail;

mod cmds;
mod utils;

fn main() -> anyhow::Result<()> {
    let args = get_args();
    let (cmd, cmd_args) = split_cmd_and_args(&args)?;

    match cmd {
        "get-file-path" => cmds::get_file_path::run(cmd_args.into_iter()),
        "get-github-file-link" => cmds::get_github_file_link::run(cmd_args.into_iter()),
        "open-editor" => cmds::open_editor::run(cmd_args.into_iter()),
        "install-dev-tools" => cmds::install_dev_tools::run(cmd_args.into_iter()),
        unknown_cmd => bail!("unknown cmd '{unknown_cmd}' in args {args:?}"),
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
