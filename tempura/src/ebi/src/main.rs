#![feature(exit_status_error)]
use anyhow::anyhow;

mod cmds;

fn main() -> anyhow::Result<()> {
    let args = utils::get_args();
    let (cmd, args) = utils::split_cmd_and_args(&args)?;
    utils::load_additional_paths()?;

    match cmd {
        "yhfp" => cmds::yhfp::run(args.into_iter()),
        "yghfl" => cmds::yghfl::run(args.into_iter()),
        "idt" => cmds::idt::run(args.into_iter()),
        unknown_cmd => Err(anyhow!("unknown cmd '{unknown_cmd}' in args {args:?}")),
    }
}
