//! Install language servers, linters, formatters and helpers concurrently.
#![feature(exit_status_error)]

use std::path::Path;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use color_eyre::owo_colors::OwoColorize as _;

use crate::installers::Installer;
use crate::installers::bash_language_server::BashLanguageServer;
use crate::installers::commitlint::Commitlint;
use crate::installers::deno::Deno;
use crate::installers::docker_langserver::DockerLangServer;
use crate::installers::elixir_ls::ElixirLs;
use crate::installers::elm_language_server::ElmLanguageServer;
use crate::installers::eslint_d::EslintD;
use crate::installers::graphql_lsp::GraphQlLsp;
use crate::installers::hadolint::Hadolint;
use crate::installers::harper_ls::HarperLs;
use crate::installers::helm_ls::HelmLs;
use crate::installers::lua_ls::LuaLanguageServer;
use crate::installers::marksman::Marksman;
use crate::installers::nvim::Nvim;
use crate::installers::prettierd::PrettierD;
use crate::installers::quicktype::Quicktype;
use crate::installers::ruff_lsp::RuffLsp;
use crate::installers::rust_analyzer::RustAnalyzer;
use crate::installers::shellcheck::Shellcheck;
use crate::installers::sql_language_server::SqlLanguageServer;
use crate::installers::sqruff::Sqruff;
use crate::installers::taplo::Taplo;
use crate::installers::terraform_ls::TerraformLs;
use crate::installers::typescript_language_server::TypescriptLanguageServer;
use crate::installers::typos_lsp::TyposLsp;
use crate::installers::vscode_langservers::VsCodeLangServers;
use crate::installers::yaml_language_server::YamlLanguageServer;

mod downloaders;
mod installers;

/// Installs development tools including language servers and utilities.
///
/// # Usage
///
/// ```bash
/// idt <dev_tools_dir> <bin_dir>             # install ALL supported tools
/// idt <dev_tools_dir> <bin_dir> deno ruff_lsp # install only listed tools
/// ```
///
/// # Arguments
///
/// * `dev_tools_dir` - Directory for tool installation (created if missing)
/// * `bin_dir` - Directory for binary symlinks (created if missing)
/// * `tool_names` - Optional specific tools to install (defaults to all)
///
/// # Errors
///
/// Returns an error if:
/// - A required argument (dev_tools_dir or bin_dir) is missing.
/// - Creating a required directory fails.
/// - Authenticating with the GitHub CLI fails.
/// - An installer thread panics.
/// - A tool installation fails (individual installer reports the specific cause).
/// - Removing dead symlinks in bin_dir fails.
#[allow(clippy::too_many_lines)]
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    println!(
        "{:#?} started with args {}",
        std::env::current_exe()?.bold().cyan(),
        format!("{args:#?}").bold()
    );

    let dev_tools_dir = args
        .first()
        .ok_or_else(|| eyre!("missing dev_tools_dir arg from {args:#?}"))?
        .trim_end_matches('/');
    let bin_dir = args
        .get(1)
        .ok_or_else(|| eyre!("missing bin_dir arg from {args:#?}"))?
        .trim_end_matches('/');
    let supplied_bin_names: Vec<&str> = args.iter().skip(2).map(AsRef::as_ref).collect();

    std::fs::create_dir_all(dev_tools_dir)?;
    std::fs::create_dir_all(bin_dir)?;

    ytil_github::log_into_github()?;

    let all_installers: Vec<Box<dyn Installer>> = vec![
        Box::new(BashLanguageServer {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(Commitlint {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(Deno {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(DockerLangServer {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(ElixirLs {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(ElmLanguageServer {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(EslintD {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(GraphQlLsp {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(Hadolint {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(HarperLs {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(HelmLs {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(LuaLanguageServer {
            dev_tools_dir: Path::new(dev_tools_dir),
        }),
        Box::new(Marksman {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(Nvim {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(PrettierD {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(Quicktype {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(RuffLsp {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(RustAnalyzer {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(Shellcheck {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(Sqruff {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(SqlLanguageServer {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(Taplo {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(TerraformLs {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(TypescriptLanguageServer {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(TyposLsp {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(VsCodeLangServers {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(YamlLanguageServer {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
    ];

    let (selected_installers, unknown_bin_names): (Vec<_>, Vec<_>) = if supplied_bin_names.is_empty() {
        (all_installers.iter().collect(), vec![])
    } else {
        let mut selected_installers = vec![];
        let mut unknown_installers = vec![];
        for chosen_bin in supplied_bin_names {
            if let Some(i) = all_installers.iter().find(|i| chosen_bin == i.bin_name()) {
                selected_installers.push(i);
            } else {
                unknown_installers.push(chosen_bin);
            }
        }
        (selected_installers, unknown_installers)
    };

    if !unknown_bin_names.is_empty() {
        eprintln!(
            "{} bins without matching installers",
            format!("{unknown_bin_names:#?}").yellow().bold()
        );
    }

    let installers_errors = std::thread::scope(|scope| {
        selected_installers
            .iter()
            .map(|installer| (installer.bin_name(), scope.spawn(move || installer.run())))
            .fold(vec![], |mut acc, (bin_name, handle)| {
                if let Err(error) = handle.join() {
                    eprintln!(
                        "{} installer thread panicked, {}",
                        bin_name.bold().red(),
                        format!("error {error:#?}").red().bold()
                    );
                    acc.push((bin_name, error));
                }
                acc
            })
    });

    ytil_system::rm_dead_symlinks(bin_dir)?;

    let mut errors_count: usize = 0;
    let mut bin_names = vec![];
    for (bin_name, _) in &installers_errors {
        errors_count = errors_count.saturating_add(1);
        bin_names.push(bin_name);
    }

    if errors_count != 0 {
        // This is a general report about the installation process.
        // The single installation errors are reported directly in each [Installer].
        bail!("{errors_count} bins failed to install, namely: {bin_names:#?}")
    }

    Ok(())
}
