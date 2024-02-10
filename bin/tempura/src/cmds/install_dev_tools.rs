use std::fmt::Debug;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use anyhow::anyhow;
use anyhow::bail;

pub fn run<'a>(mut args: impl Iterator<Item = &'a str> + Debug) -> anyhow::Result<()> {
    let dev_tools_dir = args
        .next()
        .ok_or_else(|| anyhow!("missing dev_tools_dir arg from {args:?}"))?
        .trim_end_matches('/');
    let bin_dir = args
        .next()
        .ok_or_else(|| anyhow!("missing bin_dir arg from {args:?}"))?
        .trim_end_matches('/');

    std::fs::create_dir_all(dev_tools_dir)?;
    std::fs::create_dir_all(bin_dir)?;

    // log_into_github()?;
    //
    // curl_install(
    //     "https://github.com/rust-lang/rust-analyzer/releases/download/nightly/rust-analyzer-aarch64-apple-darwin.gz",
    //     OutputOption::UnpackVia(Command::new("zcat"), &format!("{bin_dir}/rust-analyzer"))
    // )?;
    //
    // // get_bin_via_curl(
    // //     "https://github.com/tamasfe/taplo/releases/latest/download/taplo-full-darwin-aarch64.gz",
    // //     OutputOption::UnpackVia(Command::new("zcat"), &format!("{bin_dir}/taplo")),
    // // )?;
    //
    // let repo = "hashicorp/terraform-ls";
    // let latest_release = &get_latest_release(repo)?[1..];
    // curl_install(
    //     &format!("https://releases.hashicorp.com/terraform-ls/{latest_release}/terraform-ls_{latest_release}_darwin_arm64.zip"),
    //     OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    // )?;
    //
    // let repo = "tekumara/typos-vscode";
    // let latest_release = get_latest_release(repo)?;
    // curl_install(
    //     &format!("https://github.com/{repo}/releases/download/{latest_release}/typos-lsp-{latest_release}-aarch64-apple-darwin.tar.gz"),
    //     OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    // )?;
    //
    // let repo = "errata-ai/vale";
    // let latest_release = get_latest_release(repo)?;
    // curl_install(
    //     &format!("https://github.com/{repo}/releases/download/{latest_release}/vale_{}_macOS_arm64.tar.gz", latest_release[1..].to_owned()),
    //     OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    // )?;
    //
    // curl_install(
    //     "https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Darwin-x86_64",
    //     OutputOption::WriteTo(&format!("{bin_dir}/hadolint")),
    // )?;
    //
    // curl_install(
    //     "https://github.com/mrjosh/helm-ls/releases/latest/download/helm_ls_darwin_amd64",
    //     OutputOption::WriteTo(&format!("{bin_dir}/helm_ls")),
    // )?;
    //
    // curl_install(
    //     "https://github.com/artempyanykh/marksman/releases/latest/download/marksman-macos",
    //     OutputOption::WriteTo(&format!("{bin_dir}/marksman")),
    // )?;
    //
    // let tool = "shellcheck";
    // let repo = format!("koalaman/{tool}");
    // let latest_release = get_latest_release(&repo)?;
    // curl_install(
    //     &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}.darwin.x86_64.tar.xz"),
    //     OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", "/tmp"])),
    // )?;
    // let exit_status = Command::new("mv")
    //     .args([&format!("/tmp/{tool}-{latest_release}/{tool}"), bin_dir])
    //     .status()?;
    // if !exit_status.success() {
    //     bail!("error moving /tmp/{tool} to {bin_dir}")
    // }
    //
    // let tool = "elixir-ls";
    // let repo = format!("elixir-lsp/{tool}");
    // let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");
    // let latest_release = get_latest_release(&repo)?;
    // std::fs::create_dir_all(&dev_tools_repo_dir)?;
    // curl_install(
    //     &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}.zip"),
    //     OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
    // )?;
    // chmod_x(&format!("{dev_tools_repo_dir}/*"))?;
    // let exit_status = Command::new("ln")
    //     .args([
    //         "-sf",
    //         &format!("{dev_tools_repo_dir}/language_server.sh"),
    //         &format!("{bin_dir}/elixir-ls"),
    //     ])
    //     .status()?;
    // if !exit_status.success() {
    //     bail!("error symlinking {dev_tools_repo_dir}/language_server.sh to {bin_dir}/elixir-ls")
    // }
    //
    // // No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to point to
    // // the `bin` there.
    // let tool = "lua-language-server";
    // let repo = format!("LuaLS/{tool}");
    // let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");
    // let latest_release = get_latest_release(&repo)?;
    // std::fs::create_dir_all(&dev_tools_repo_dir)?;
    // curl_install(
    //     &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}-darwin-arm64.tar.gz"),
    //     OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
    // )?;
    //
    // composer_install(
    //     dev_tools_dir,
    //     "phpactor",
    //     "phpactor/phpactor",
    //     bin_dir,
    //     "phpactor",
    // )?;
    //
    // composer_install(
    //     dev_tools_dir,
    //     "php-cs-fixer",
    //     "friendsofphp/php-cs-fixer",
    //     bin_dir,
    //     "php-cs-fixer",
    // )?;
    //
    // composer_install(dev_tools_dir, "psalm", "vimeo/psalm", bin_dir, "*")?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "commitlint",
    //     &["@commitlint/cli", "@commitlint/config-conventional"],
    //     bin_dir,
    //     "commitlint",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "elm-language-server",
    //     &["@elm-tooling/elm-language-server"],
    //     bin_dir,
    //     "elm-language-server",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "compose-language-service",
    //     &["@microsoft/compose-language-service"],
    //     bin_dir,
    //     "docker-compose-langserver",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "bash-language-server",
    //     &["bash-language-server"],
    //     bin_dir,
    //     "bash-language-server",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "dockerfile-language-server-nodejs",
    //     &["dockerfile-language-server-nodejs"],
    //     bin_dir,
    //     "docker-langserver",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "eslint_d",
    //     &["eslint_d"],
    //     bin_dir,
    //     "eslint_d",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "graphql-language-service-cli",
    //     &["graphql-language-service-cli"],
    //     bin_dir,
    //     "graphql-lsp",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "prettierd",
    //     &["@fsouza/prettierd"],
    //     bin_dir,
    //     "prettierd",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "sql-language-server",
    //     &["sql-language-server"],
    //     bin_dir,
    //     "sql-language-server",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "vscode-langservers-extracted",
    //     &["vscode-langservers-extracted"],
    //     bin_dir,
    //     "*",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "yaml-language-server",
    //     &["yaml-language-server"],
    //     bin_dir,
    //     "yaml-language-server",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "typescript-language-server",
    //     &["typescript-language-server", "typescript"],
    //     bin_dir,
    //     "typescript-language-server",
    // )?;
    //
    // npm_install(
    //     dev_tools_dir,
    //     "quicktype",
    //     &["quicktype"],
    //     bin_dir,
    //     "quicktype",
    // )?;

    pip_install(
        dev_tools_dir,
        "ruff-lsp",
        &["ruff-lsp"],
        bin_dir,
        "ruff-lsp",
    )?;

    pip_install(
        dev_tools_dir,
        "sqlfluff",
        &["sqlfluff"],
        bin_dir,
        "sqlfluff",
    )?;

    // chmod_x(&format!("{bin_dir}/*"))?;

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

fn curl_install(url: &str, output_option: OutputOption) -> anyhow::Result<()> {
    let mut curl_cmd = Command::new("curl");
    curl_cmd.args(["-SL", url]);

    match output_option {
        OutputOption::UnpackVia(mut cmd, output_path) => {
            let curl_stdout = curl_cmd
                .stdout(Stdio::piped())
                .spawn()?
                .stdout
                .ok_or_else(|| anyhow!("missing stdout from cmd {curl_cmd:?}"))?;
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
                .ok_or_else(|| anyhow!("missing stdout from cmd {curl_cmd:?}"))?;
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

fn composer_install(
    dev_tools_dir: &str,
    tool: &str,
    package: &str,
    bin_dir: &str,
    bin: &str,
) -> anyhow::Result<()> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    Command::new("composer")
        .args([
            "require",
            "--dev",
            "--working-dir",
            &dev_tools_repo_dir,
            package,
        ])
        .status()?;

    Command::new("sh")
        .args([
            "-c",
            &format!("ln -sf {dev_tools_repo_dir}/vendor/bin/{bin} {bin_dir}"),
        ])
        .spawn()?
        .wait()?;

    Ok(())
}

fn npm_install(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dir: &str,
    bin: &str,
) -> anyhow::Result<()> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    Command::new("npm")
        .args(
            [
                &["install", "--silent", "--prefix", &dev_tools_repo_dir][..],
                packages,
            ]
            .concat(),
        )
        .status()?;

    Command::new("sh")
        .args([
            "-c",
            &format!("ln -sf {dev_tools_repo_dir}/node_modules/.bin/{bin} {bin_dir}"),
        ])
        .spawn()?
        .wait()?;

    Ok(())
}

fn pip_install(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dir: &str,
    bin: &str,
) -> anyhow::Result<()> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    Command::new("python3")
        .args(["-m", "venv", &format!("{dev_tools_repo_dir}/.venv")])
        .spawn()?
        .wait()?;

    Command::new("sh")
        .args([
            "-c",
            &format!(
                r#"
                    source {dev_tools_repo_dir}/.venv/bin/activate && \
                    pip install pip {packages} --upgrade  && \
                    ln -sf {dev_tools_repo_dir}/.venv/bin/{bin} {bin_dir}
                "#,
                packages = packages.join(" "),
            ),
        ])
        .spawn()?
        .wait()?;

    Ok(())
}

// Yes, `dir` is a `&str` and it's not sanitized but...I'm the alpha & the omega here!
fn chmod_x(dir: &str) -> anyhow::Result<()> {
    let exit_status = Command::new("sh")
        .args(["-c", &format!("chmod +x {dir}")])
        .status()?;
    if !exit_status.success() {
        bail!("error setting executable permission to {dir}")
    }
    Ok(())
}
