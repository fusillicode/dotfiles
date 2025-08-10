use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use color_eyre::eyre::eyre;

pub enum OutputOption<'a> {
    UnpackVia(Box<Command>, &'a str),
    PipeInto(&'a mut Command, String),
    WriteTo(&'a str),
}

pub fn run(url: &str, output_option: OutputOption) -> color_eyre::Result<String> {
    let mut curl_cmd = utils::cmd::silent_cmd("curl");
    let silent_flag = cfg!(debug_assertions).then(|| "S").unwrap_or("");
    curl_cmd.args([&format!("-L{silent_flag}"), url]);

    Ok(match output_option {
        OutputOption::UnpackVia(mut cmd, bin_dest) => {
            let curl_stdout = curl_cmd
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| eyre!("missing stdout from cmd {curl_cmd:#?}"))?;
            let output = cmd.stdin(Stdio::from(curl_stdout)).output()?;
            output.status.exit_ok()?;
            let mut file = File::create(bin_dest)?;
            file.write_all(&output.stdout)?;
            bin_dest.into()
        }
        OutputOption::PipeInto(cmd, bin_dest) => {
            let curl_stdout = curl_cmd
                .arg(&bin_dest)
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| eyre!("missing stdout from cmd {curl_cmd:#?}"))?;
            cmd.stdin(Stdio::from(curl_stdout)).status()?.exit_ok()?;
            bin_dest
        }
        OutputOption::WriteTo(bin_dest) => {
            curl_cmd.arg("--output");
            curl_cmd.arg(bin_dest);
            curl_cmd.status()?.exit_ok()?;
            bin_dest.into()
        }
    })
}
