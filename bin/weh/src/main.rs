use anyhow::anyhow;
use anyhow::bail;

mod cmds;
mod utils;

fn main() -> anyhow::Result<()> {
    let args = get_args();
    let (cmd, cmd_args) = split_cmd_and_args(&args)?;

    match cmd {
        "ghl" => cmds::ghl::run(cmd_args.into_iter()),
        "ho" => cmds::ho::run(cmd_args.into_iter()),
        unknown_cmd => bail!("unknown cmd {unknown_cmd} in args {args:?}"),
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
