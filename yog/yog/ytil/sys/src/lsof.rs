use std::process::Command;

use color_eyre::eyre::Context as _;
use color_eyre::eyre::eyre;
use itertools::Itertools as _;
use ytil_cmd::CmdExt as _;

#[derive(Debug)]
pub enum ProcessFilter<'a> {
    Pid(&'a str),
    Name(&'a str),
}

#[derive(Debug)]
pub struct ProcessDescription {
    pub pid: String,
    pub cwd: String,
}

pub fn lsof(process_filter: &ProcessFilter) -> color_eyre::Result<Vec<ProcessDescription>> {
    let cmd = "lsof";

    let process_filter = match process_filter {
        ProcessFilter::Pid(pid) => ["-p", pid],
        ProcessFilter::Name(name) => ["-c", name],
    };
    let mut args = vec!["-F", "n"];
    args.extend(process_filter);
    args.extend(["-a", "-d", "cwd"]);

    let stdout = Command::new(cmd)
        .args(&args)
        .exec()
        .wrap_err_with(|| eyre!("error running cmd | cmd={cmd:?} args={args:?}"))?
        .exit_ok()
        .wrap_err_with(|| eyre!("error cmd exit not ok | cmd={cmd:?} args={args:?}"))?
        .stdout;

    let output = str::from_utf8(&stdout)?;
    parse_lsof_output(output)
}

fn parse_lsof_output(output: &str) -> color_eyre::Result<Vec<ProcessDescription>> {
    let mut out = vec![];
    // The hardcoded 3 is tight to the lsof args.
    // Changes to lsof args will have impact on the chunks size.
    for mut line in &output.lines().chunks(3) {
        let pid = line
            .next()
            .ok_or_else(|| eyre!("error missing pid in lsof line"))?
            .trim_start_matches('p');
        line.next().ok_or_else(|| eyre!("error missing f in lsof line"))?;
        let cwd = line
            .next()
            .ok_or_else(|| eyre!("error missing cwd in lsof line"))?
            .trim_start_matches('n');
        out.push(ProcessDescription {
            pid: pid.to_owned(),
            cwd: cwd.to_owned(),
        });
    }
    Ok(out)
}
