mod cmds;
mod utils;

fn main() -> anyhow::Result<()> {
    let args = get_args();
    let (cmd, args) = parse_cmd_and_args(&args);

    match cmd {
        "gh" => cmds::gh::run(args.into_iter()),
        "ho" => cmds::ho::run(args.into_iter()),
        unexpected_cmd => anyhow::bail!("BOOM {} {:?}", unexpected_cmd, args),
    }
}

fn get_args() -> Vec<String> {
    let mut x = std::env::args();
    x.next();
    x.collect::<Vec<String>>()
}

fn parse_cmd_and_args(args: &[String]) -> (&str, Vec<&str>) {
    args.split_first()
        .map(|(cmd, cmd_args)| (cmd.as_str(), cmd_args.iter().map(String::as_str).collect()))
        .unwrap()
}
