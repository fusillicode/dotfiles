use std::fmt::Debug;
use std::process::Command;

use anyhow::anyhow;

use composer_install::composer_install;
use curl_install::curl_install;
use curl_install::OutputOption;
use npm_install::npm_install;
use pip_install::pip_install;

use crate::utils::github::get_latest_release;
use crate::utils::github::log_into_github;
use crate::utils::system::chmod_x;

mod composer_install;
mod curl_install;
mod npm_install;
mod pip_install;

// TODO: maybe this is enough to abstract installers 🤔
// type Installer = Box<dyn Fn(&str, &str) -> anyhow::Result<()>>;

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

    log_into_github()?;

    // Compiling from sources because I can checkout specific refs in case of broken nightly builds.
    // Moreover...it's pretty badass 😎
    let nvim_source_dir = format!("{dev_tools_dir}/nvim/source");
    let nvim_release_dir = format!("{dev_tools_dir}/nvim/release");
    Command::new("sh")
        .args([
            "-c",
            &format!(
                r#"
                    ([ ! -d "{nvim_source_dir}" ] && \
                        git clone https://github.com/neovim/neovim {nvim_source_dir} || true) && \
                    cd {nvim_source_dir} && \
                    git checkout master && \
                    git pull origin master && \
                    make distclean && \
                    make CMAKE_BUILD_TYPE=Release CMAKE_EXTRA_FLAGS="-DCMAKE_INSTALL_PREFIX={nvim_release_dir}" && \
                    make install
                    ln -sf {nvim_release_dir}/bin/nvim {bin_dir}
                "#,
            ),
        ])
        .status()?
        .exit_ok()?;

    curl_install(
        "https://github.com/rust-lang/rust-analyzer/releases/download/nightly/rust-analyzer-aarch64-apple-darwin.gz",
        OutputOption::UnpackVia(Command::new("zcat"), &format!("{bin_dir}/rust-analyzer"))
    )?;

    // Installing with `cargo` because of:
    // 1. no particular requirements
    // 2. https://github.com/tamasfe/taplo/issues/542
    Command::new("cargo")
        .args([
            "install",
            "taplo-cli",
            "--force",
            "--all-features",
            "--root",
            // `--root` automatically append `bin` 🥲
            bin_dir.trim_end_matches("bin"),
        ])
        .status()?;

    let repo = "hashicorp/terraform-ls";
    let latest_release = &get_latest_release(repo)?[1..];
    curl_install(
        &format!("https://releases.hashicorp.com/terraform-ls/{latest_release}/terraform-ls_{latest_release}_darwin_arm64.zip"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )?;

    // For Markdown preview with peek.nvim
    let repo = "denoland/deno";
    let latest_release = get_latest_release(repo)?;
    curl_install(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/deno-aarch64-apple-darwin.zip"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )?;

    let repo = "tekumara/typos-vscode";
    let latest_release = get_latest_release(repo)?;
    curl_install(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/typos-lsp-{latest_release}-aarch64-apple-darwin.tar.gz"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )?;

    let repo = "errata-ai/vale";
    let latest_release = get_latest_release(repo)?;
    curl_install(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/vale_{}_macOS_arm64.tar.gz", latest_release[1..].to_owned()),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )?;

    curl_install(
        "https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Darwin-x86_64",
        OutputOption::WriteTo(&format!("{bin_dir}/hadolint")),
    )?;

    curl_install(
        "https://github.com/mrjosh/helm-ls/releases/latest/download/helm_ls_darwin_amd64",
        OutputOption::WriteTo(&format!("{bin_dir}/helm_ls")),
    )?;

    curl_install(
        "https://github.com/artempyanykh/marksman/releases/latest/download/marksman-macos",
        OutputOption::WriteTo(&format!("{bin_dir}/marksman")),
    )?;

    let tool = "shellcheck";
    let repo = format!("koalaman/{tool}");
    let latest_release = get_latest_release(&repo)?;
    curl_install(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}.darwin.x86_64.tar.xz"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", "/tmp"])),
    )?;
    Command::new("mv")
        .args([&format!("/tmp/{tool}-{latest_release}/{tool}"), bin_dir])
        .status()?
        .exit_ok()?;

    let tool = "elixir-ls";
    let repo = format!("elixir-lsp/{tool}");
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");
    let latest_release = get_latest_release(&repo)?;
    std::fs::create_dir_all(&dev_tools_repo_dir)?;
    curl_install(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}.zip"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
    )?;
    chmod_x(&format!("{dev_tools_repo_dir}/*"))?;
    Command::new("ln")
        .args([
            "-sf",
            &format!("{dev_tools_repo_dir}/language_server.sh"),
            &format!("{bin_dir}/elixir-ls"),
        ])
        .status()?
        .exit_ok()?;

    // No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to point to
    // the `bin` there.
    let tool = "lua-language-server";
    let repo = format!("LuaLS/{tool}");
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");
    let latest_release = get_latest_release(&repo)?;
    std::fs::create_dir_all(&dev_tools_repo_dir)?;
    curl_install(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}-darwin-arm64.tar.gz"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
    )?;

    composer_install(
        dev_tools_dir,
        "phpactor",
        &["phpactor/phpactor"],
        bin_dir,
        "phpactor",
    )?;

    composer_install(
        dev_tools_dir,
        "php-cs-fixer",
        &["friendsofphp/php-cs-fixer"],
        bin_dir,
        "php-cs-fixer",
    )?;

    composer_install(dev_tools_dir, "psalm", &["vimeo/psalm"], bin_dir, "*")?;

    npm_install(
        dev_tools_dir,
        "commitlint",
        &["@commitlint/cli", "@commitlint/config-conventional"],
        bin_dir,
        "commitlint",
    )?;

    npm_install(
        dev_tools_dir,
        "elm-language-server",
        &["@elm-tooling/elm-language-server"],
        bin_dir,
        "elm-language-server",
    )?;

    npm_install(
        dev_tools_dir,
        "bash-language-server",
        &["bash-language-server"],
        bin_dir,
        "bash-language-server",
    )?;

    npm_install(
        dev_tools_dir,
        "dockerfile-language-server-nodejs",
        &["dockerfile-language-server-nodejs"],
        bin_dir,
        "docker-langserver",
    )?;

    npm_install(
        dev_tools_dir,
        "eslint_d",
        &["eslint_d"],
        bin_dir,
        "eslint_d",
    )?;

    npm_install(
        dev_tools_dir,
        "graphql-language-service-cli",
        &["graphql-language-service-cli"],
        bin_dir,
        "graphql-lsp",
    )?;

    npm_install(
        dev_tools_dir,
        "prettierd",
        &["@fsouza/prettierd"],
        bin_dir,
        "prettierd",
    )?;

    npm_install(
        dev_tools_dir,
        "sql-language-server",
        &["sql-language-server"],
        bin_dir,
        "sql-language-server",
    )?;

    npm_install(
        dev_tools_dir,
        "vscode-langservers-extracted",
        &["vscode-langservers-extracted"],
        bin_dir,
        "*",
    )?;

    npm_install(
        dev_tools_dir,
        "yaml-language-server",
        &["yaml-language-server"],
        bin_dir,
        "yaml-language-server",
    )?;

    npm_install(
        dev_tools_dir,
        "typescript-language-server",
        &["typescript-language-server", "typescript"],
        bin_dir,
        "typescript-language-server",
    )?;

    npm_install(
        dev_tools_dir,
        "quicktype",
        &["quicktype"],
        bin_dir,
        "quicktype",
    )?;

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

    chmod_x(&format!("{bin_dir}/*"))?;

    Ok(())
}
