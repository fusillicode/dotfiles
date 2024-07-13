use anyhow::anyhow;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use crate::utils::system::silent_cmd;

pub enum OutputOption<'a> {
    UnpackVia(Command, &'a str),
    PipeInto(&'a mut Command),
    WriteTo(&'a str),
}

pub fn run(url: &str, output_option: OutputOption) -> anyhow::Result<()> {
    let mut curl_cmd = silent_cmd("curl");
    curl_cmd.args(["-SL", url]);

    match output_option {
        OutputOption::UnpackVia(mut cmd, output_path) => {
            let curl_stdout = curl_cmd
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| anyhow!("missing stdout from cmd {curl_cmd:?}"))?;
            let output = cmd.stdin(Stdio::from(curl_stdout)).output()?;
            output.status.exit_ok()?;

            let mut file = File::create(output_path)?;
            Ok(file.write_all(&output.stdout)?)
        }
        OutputOption::PipeInto(cmd) => {
            let curl_stdout = curl_cmd
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| anyhow!("missing stdout from cmd {curl_cmd:?}"))?;

            Ok(cmd.stdin(Stdio::from(curl_stdout)).status()?.exit_ok()?)
        }
        OutputOption::WriteTo(output_path) => {
            curl_cmd.arg("--output");
            curl_cmd.arg(output_path);

            Ok(curl_cmd.status()?.exit_ok()?)
        }
    }
}
