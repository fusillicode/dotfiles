use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use anyhow::anyhow;
use anyhow::bail;

pub fn run<'a>(mut args: impl Iterator<Item = &'a str> + std::fmt::Debug) -> anyhow::Result<()> {
    let dev_tools_dir = args
        .next()
        .ok_or_else(|| anyhow!("missing dev_tools_dir arg from {args:?}"))?;
    let bin_dir = args
        .next()
        .ok_or_else(|| anyhow!("missing bin_dir arg from {args:?}"))?;

    std::fs::create_dir_all(dev_tools_dir)?;
    std::fs::create_dir_all(bin_dir)?;

    log_into_github()?;

    get_bin_via_curl(
        "https://github.com/rust-lang/rust-analyzer/releases/download/nightly/rust-analyzer-aarch64-apple-darwin.gz",
        OutputOption::UnpackVia(Command::new("zcat"), &format!("{bin_dir}/rust-analyzer"))
    )?;

    // get_bin_via_curl(
    //     "https://github.com/tamasfe/taplo/releases/latest/download/taplo-full-darwin-aarch64.gz",
    //     OutputOption::UnpackVia(Command::new("zcat"), &format!("{bin_dir}/taplo")),
    // )?;

    let repo = "hashicorp/terraform-ls";
    let latest_release = &get_latest_release(repo)?[1..];
    get_bin_via_curl(
        &format!("https://releases.hashicorp.com/terraform-ls/{latest_release}/terraform-ls_{latest_release}_darwin_arm64.zip"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )?;

    let repo = "tekumara/typos-vscode";
    let latest_release = get_latest_release(repo)?;
    get_bin_via_curl(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/typos-lsp-{latest_release}-aarch64-apple-darwin.tar.gz"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )?;

    let repo = "errata-ai/vale";
    let latest_release = get_latest_release(repo)?;
    get_bin_via_curl(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/vale_{0}_macOS_arm64.tar.gz ", latest_release[1..].to_owned()),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )?;

    get_bin_via_curl(
        "https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Darwin-x86_64",
        OutputOption::WriteTo(&format!("{bin_dir}/hadolint")),
    )?;

    get_bin_via_curl(
        "https://github.com/mrjosh/helm-ls/releases/latest/download/helm_ls_darwin_amd64",
        OutputOption::WriteTo(&format!("{bin_dir}/helm_ls")),
    )?;

    get_bin_via_curl(
        "https://github.com/artempyanykh/marksman/releases/latest/download/marksman-macos",
        OutputOption::WriteTo(&format!("{bin_dir}/marksman")),
    )?;

    let tool = "shellcheck";
    let repo = format!("koalaman/{tool}");
    let latest_release = get_latest_release(&repo)?;
    get_bin_via_curl(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}.darwin.x86_64.tar.xz"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", "/tmp"])),
    )?;
    let exit_status = Command::new("mv")
        .args([&format!("/tmp/{tool}"), bin_dir])
        .status()?;
    if !exit_status.success() {
        bail!("error setting moving /tmp/{tool} to {bin_dir}")
    }

    let tool = "elixirls";
    let repo = format!("elixir-lsp/{tool}");
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");
    let latest_release = get_latest_release(&repo)?;
    get_bin_via_curl(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}.zip"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
    )?;
    let exit_status = Command::new("chmod")
        .args(["+x", &format!("{dev_tools_repo_dir}/*")])
        .status()?;
    if !exit_status.success() {
        bail!("error setting executable permission for to /{dev_tools_repo_dir}/*")
    }
    std::os::unix::fs::symlink(
        format!("{dev_tools_repo_dir}/language_server.sh"),
        format!("{bin_dir}/elixir-ls"),
    )?;
    // tool="elixir-ls"
    // repo="elixir-lsp/$tool"
    // dev_tools_repo_dir="$dev_tools_dir"/"$tool"
    // latest_release=$(get_latest_release $repo)
    // mkdir -p "$dev_tools_repo_dir" && \
    //   curl -SL https://github.com/"$repo"/releases/download/"$latest_release"/"$tool"-"$latest_release".zip | \
    //   tar -xz -C "$dev_tools_repo_dir" && \
    //   chmod +x "$dev_tools_repo_dir"/* && \
    //   ln -sf "$dev_tools_repo_dir"/language_server.sh "$bin_dir"/elixir-ls
    //   ln -sf "$dev_tools_repo_dir"/debug_adapter.sh "$bin_dir"/elixir-ls-debugger

    let exit_status = Command::new("chmod")
        .args(["+x", &format!("{bin_dir}/*")])
        .status()?;

    if !exit_status.success() {
        bail!("error setting executable permission to {bin_dir}")
    }

    Ok(())
}

fn log_into_github() -> anyhow::Result<()> {
    if Command::new("gh")
        .args(["auth", "status"])
        .status()?
        .success()
    {
        return Ok(());
    }

    // Spawning a new shell because `gh` should block until the user is authenticated
    let exit_status = Command::new("sh")
        .args(["-c", "gh auth login"])
        .spawn()?
        .wait()?;

    if exit_status.success() {
        return Ok(());
    }

    bail!("error logging into GitHub, exit status: {exit_status:?}")
}

fn get_latest_release(repo: &str) -> anyhow::Result<String> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{repo}/releases/latest"),
            "--jq=.tag_name",
        ])
        .output()?;

    if output.status.success() {
        return Ok(std::str::from_utf8(&output.stdout)?.trim().into());
    }

    bail!("error getting latest release for repo {repo:?}, cmd output {output:?}")
}

enum OutputOption<'a> {
    UnpackVia(Command, &'a str),
    PipeInto(&'a mut Command),
    WriteTo(&'a str),
}

fn get_bin_via_curl(url: &str, output_option: OutputOption) -> anyhow::Result<()> {
    let mut curl_cmd = Command::new("curl");
    curl_cmd.args(["-SL", url]);

    match output_option {
        OutputOption::UnpackVia(mut cmd, output_path) => {
            let curl_stdout = curl_cmd
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| anyhow!("missing stdout from curl cmd {curl_cmd:?}"))?;
            let output = cmd.stdin(Stdio::from(curl_stdout)).output()?;
            if output.status.success() {
                let mut file = File::create(output_path)?;
                file.write_all(&output.stdout)?;
                return Ok(());
            }
            bail!(
                "error handling curl output by cmd {cmd:?} to write to path {output_path:?}, exit status: {0:?}",
                output.status
            )
        }
        OutputOption::PipeInto(cmd) => {
            let curl_stdout = curl_cmd
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| anyhow!("missing stdout from curl cmd {curl_cmd:?}"))?;
            let exit_status = cmd.stdin(Stdio::from(curl_stdout)).status()?;
            if exit_status.success() {
                return Ok(());
            }
            bail!("error handling curl output by cmd {cmd:?}, exit status: {exit_status:?}")
        }
        OutputOption::WriteTo(output_path) => {
            curl_cmd.arg("--output");
            curl_cmd.arg(output_path);
            let exit_status = curl_cmd.status()?;
            if exit_status.success() {
                return Ok(());
            }
            bail!("error getting bin via cmd {curl_cmd:?}, exit status: {exit_status:?}")
        }
    }
}
