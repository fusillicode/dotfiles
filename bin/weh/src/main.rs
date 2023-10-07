mod cmds;
mod utils;

fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<String>>();
    let (_, args) = args.split_first().unwrap();

    let (cmd, args) = args
        .split_first()
        .map(|(cmd, rest)| (cmd.as_str(), rest.iter().map(String::as_str)))
        .unwrap();

    match cmd {
        "gh" => cmds::gh::run(args),
        "ho" => cmds::ho::run(args),
        unexpected_cmd => anyhow::bail!("BOOM {} {:?}", unexpected_cmd, args.collect::<Vec<_>>()),
    }
}
