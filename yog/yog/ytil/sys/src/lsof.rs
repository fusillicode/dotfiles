use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use itertools::Itertools as _;
use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_cmd::CmdExt as _;

#[derive(Debug)]
pub enum ProcessFilter<'a> {
    Pid(&'a str),
    Name(&'a str),
}

#[derive(Debug)]
pub struct ProcessDescription {
    pub pid: String,
    pub cwd: PathBuf,
}

/// Retrieves process descriptions using the lsof command.
///
/// # Errors
/// - lsof command execution or output parsing fails.
pub fn lsof(process_filter: &ProcessFilter) -> rootcause::Result<Vec<ProcessDescription>> {
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
        .context("error running cmd")
        .attach_with(|| format!("cmd={cmd:?} args={args:?}"))?
        .exit_ok()
        .context("error cmd exit not ok")
        .attach_with(|| format!("cmd={cmd:?} args={args:?}"))?
        .stdout;

    let output = str::from_utf8(&stdout)?;
    parse_lsof_output(output)
}

fn parse_lsof_output(output: &str) -> rootcause::Result<Vec<ProcessDescription>> {
    let mut out = vec![];
    // The hardcoded 3 is tight to the lsof args.
    // Changes to lsof args will have impact on the chunks size.
    for mut line in &output.lines().chunks(3) {
        let pid = line
            .next()
            .ok_or_else(|| report!("error missing pid in lsof line"))?
            .trim_start_matches('p');
        line.next().ok_or_else(|| report!("error missing f in lsof line"))?;
        let cwd = line
            .next()
            .ok_or_else(|| report!("error missing cwd in lsof line"))?
            .trim_start_matches('n');

        out.push(ProcessDescription {
            pid: pid.to_owned(),
            cwd: PathBuf::from_str(cwd)
                .context("error constructing PathBuf from cwd")
                .attach_with(|| format!("cwd={cwd:?}"))?,
        });
    }
    Ok(out)
}
