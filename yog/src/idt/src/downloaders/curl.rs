use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use color_eyre::eyre::eyre;

pub enum CurlDownloaderOption<'a> {
    PipeIntoZcat {
        dest_path: &'a str,
    },
    PipeIntoTar {
        dest_dir: &'a str,
        // Option because not all the downloaded archives has a:
        // - stable name (i.e. shellcheck)
        // - an usable binary outside the archive (i.e. elixir_ls or lua_ls)
        // In these cases `dest_name` is set to None
        dest_name: Option<&'a str>,
    },
    WriteTo {
        dest_path: &'a str,
    },
}

pub fn run(url: &str, opt: CurlDownloaderOption) -> color_eyre::Result<String> {
    let mut curl_cmd = utils::cmd::silent_cmd("curl");
    let silent_flag = cfg!(debug_assertions).then(|| "S").unwrap_or("");
    curl_cmd.args([&format!("-L{silent_flag}"), url]);

    let target = match opt {
        CurlDownloaderOption::PipeIntoZcat { dest_path } => {
            let curl_stdout = get_cmd_stdout(&mut curl_cmd)?;

            let output = Command::new("zcat").stdin(curl_stdout).output()?;
            output.status.exit_ok()?;

            let mut file = File::create(dest_path)?;
            file.write_all(&output.stdout)?;

            dest_path.into()
        }
        CurlDownloaderOption::PipeIntoTar {
            dest_dir,
            dest_name,
        } => {
            let curl_stdout = get_cmd_stdout(&mut curl_cmd)?;

            let mut tar_cmd = Command::new("tar");
            tar_cmd.args(["-xz", "-C", dest_dir]);
            if let Some(dest_name) = dest_name {
                tar_cmd.arg(dest_name);
            }
            tar_cmd.stdin(curl_stdout).status()?.exit_ok()?;

            dest_name
                .map(|dn| format!("{dest_dir}/{dn}"))
                .unwrap_or_else(|| dest_dir.into())
        }
        CurlDownloaderOption::WriteTo { dest_path } => {
            curl_cmd.arg("--output");
            curl_cmd.arg(dest_path);
            curl_cmd.status()?.exit_ok()?;

            dest_path.into()
        }
    };

    Ok(target)
}

fn get_cmd_stdout(cmd: &mut Command) -> color_eyre::Result<Stdio> {
    let stdout = cmd
        .stdout(Stdio::piped())
        .spawn()?
        .stdout
        .ok_or_else(|| eyre!("missing stdout from cmd {cmd:#?}"))?;

    Ok(Stdio::from(stdout))
}
