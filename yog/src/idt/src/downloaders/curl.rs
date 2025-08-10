use std::fs::File;
use std::io::Write;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;

use color_eyre::eyre::eyre;

pub enum InstallOption<'a> {
    UnpackViaZcat {
        dest_path: &'a str,
    },
    PipeIntoTar {
        dest_dir: &'a str,
        dest_name: &'a str,
    },
    WriteTo {
        dest_path: &'a str,
    },
}

pub fn run(url: &str, opt: InstallOption) -> color_eyre::Result<()> {
    let mut curl_cmd = utils::cmd::silent_cmd("curl");
    let silent_flag = cfg!(debug_assertions).then(|| "S").unwrap_or("");
    curl_cmd.args([&format!("-L{silent_flag}"), url]);

    match opt {
        InstallOption::UnpackViaZcat { dest_path } => {
            let curl_stdout = get_stdout_from_cmd(&mut curl_cmd)?;

            let output = Command::new("zcat")
                .stdin(Stdio::from(curl_stdout))
                .output()?;
            output.status.exit_ok()?;

            let mut file = File::create(dest_path)?;
            file.write_all(&output.stdout)?;

            Ok(())
        }
        InstallOption::PipeIntoTar {
            dest_dir,
            dest_name,
        } => {
            let curl_stdout = get_stdout_from_cmd(&mut curl_cmd)?;

            Command::new("tar")
                .args(["-xz", "-C", dest_dir, dest_name])
                .stdin(Stdio::from(curl_stdout))
                .status()?
                .exit_ok()?;

            Ok(())
        }
        InstallOption::WriteTo { dest_path } => {
            curl_cmd.arg("--output");
            curl_cmd.arg(dest_path);
            curl_cmd.status()?.exit_ok()?;

            Ok(())
        }
    }
}

fn get_stdout_from_cmd(cmd: &mut Command) -> color_eyre::Result<ChildStdout> {
    let stdout = cmd
        .stdout(Stdio::piped())
        .spawn()?
        .stdout
        .ok_or_else(|| eyre!("missing stdout from cmd {cmd:#?}"))?;

    Ok(stdout)
}
