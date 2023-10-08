use anyhow::anyhow;
use anyhow::bail;

mod cmds;
mod utils;

fn main() -> anyhow::Result<()> {
    let args = get_args();
    let (cmd, args) = split_cmd_and_args(&args)?;

    match cmd {
        "gh" => cmds::gh::run(args.into_iter()),
        "ho" => cmds::ho::run(args.into_iter()),
        unknown_cmd => bail!("unknown cmd {unknown_cmd} from args {args:?}"),
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
