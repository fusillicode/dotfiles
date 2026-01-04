//! Install language servers, linters, formatters, and developer helpers concurrently.
//!
//! # Arguments
//! - `dev_tools_dir` Directory for tool installation (created if missing).
//! - `bin_dir` Directory for binary symlinks (created if missing).
//! - `tool_names` Optional specific tools to install (defaults to all).
//!
//! # Usage
//! ```bash
//! idt ~/.dev/tools ~/.local/bin # install all tools
//! idt ~/.dev/tools ~/.local/bin ruff_lsp rust_analyzer taplo # subset
//! ```
//!
//! # Flow
//! 1. Ensure target directories.
//! 2. Auth to GitHub (rate limits, releases).
//! 3. Resolve selected installers (all or subset).
//! 4. Spawn scoped threads to run installers.
//! 5. Cleanup dead symlinks; aggregate failures.
//!
//! # Errors
//! - Missing required argument (`dev_tools_dir` / `bin_dir`).
//! - Directory creation fails.
//! - GitHub authentication fails.
//! - Installer thread panics.
//! - Individual tool installation fails (installer reports detail).
//! - Dead symlink cleanup fails.
#![feature(exit_status_error)]

use std::path::Path;

use color_eyre::eyre::eyre;
use color_eyre::owo_colors::OwoColorize as _;
use ytil_sys::SysInfo;
use ytil_sys::cli::Args;

use crate::installers::Installer;
use crate::installers::bash_language_server::BashLanguageServer;
use crate::installers::commitlint::Commitlint;
use crate::installers::deno::Deno;
use crate::installers::docker_langserver::DockerLangServer;
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

/// Summarize installer thread outcomes; collect failing bin names.
///
/// # Errors
/// - Does not construct a rich error enum; instead returns failing bin names. Individual installers are expected to
///   have already printed detailed stderr output.
/// - A thread panic is logged immediately and its bin name added to the returned list.
///
/// # Rationale
/// - Simple bin-name list keeps aggregation lightweight for CI scripting.
/// - Delegates detailed formatting to installers; central function only normalizes aggregation.
/// - Easier to extend with JSON output later (convert list directly).
fn report<'a>(
    installers_res: &'a [(&'a str, std::thread::Result<color_eyre::Result<()>>)],
) -> Result<(), Vec<&'a str>> {
    let mut errors_bins = vec![];

    for (bin_name, result) in installers_res {
        match result {
            Err(err) => {
                eprintln!(
                    "{} installer thread panicked error={}",
                    bin_name.red(), // removed bold
                    format!("{err:#?}").red()
                );
                errors_bins.push(*bin_name);
            }
            Ok(Err(_)) => errors_bins.push(bin_name),
            Ok(Ok(())) => {}
        }
    }

    if errors_bins.is_empty() {
        return Ok(());
    }
    Err(errors_bins)
}

/// Install language servers, linters, formatters, and developer helpers concurrently.
#[allow(clippy::too_many_lines)]
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_sys::cli::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }
    println!(
        "{:#?} started with args {}",
        std::env::current_exe()?.bold().cyan(),
        format!("{args:#?}").white().bold()
    );

    let dev_tools_dir = args
        .first()
        .ok_or_else(|| eyre!("missing dev_tools_dir arg | args={args:#?}"))?
        .trim_end_matches('/');
    let bin_dir = args
        .get(1)
        .ok_or_else(|| eyre!("missing bin_dir arg | args={args:#?}"))?
        .trim_end_matches('/');
    let supplied_bin_names: Vec<&str> = args.iter().skip(2).map(AsRef::as_ref).collect();

    let sys_info = SysInfo::get()?;

    std::fs::create_dir_all(dev_tools_dir)?;
    std::fs::create_dir_all(bin_dir)?;

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
            sys_info: &sys_info,
        }),
        Box::new(DockerLangServer {
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
            sys_info: &sys_info,
        }),
        Box::new(HarperLs {
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(HelmLs {
            bin_dir: Path::new(bin_dir),
            sys_info: &sys_info,
        }),
        Box::new(LuaLanguageServer {
            dev_tools_dir: Path::new(dev_tools_dir),
            sys_info: &sys_info,
        }),
        Box::new(Marksman {
            bin_dir: Path::new(bin_dir),
            sys_info: &sys_info,
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
            sys_info: &sys_info,
        }),
        Box::new(Shellcheck {
            bin_dir: Path::new(bin_dir),
            sys_info: &sys_info,
        }),
        Box::new(Sqruff {
            bin_dir: Path::new(bin_dir),
            sys_info: &sys_info,
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
            sys_info: &sys_info,
        }),
        Box::new(TypescriptLanguageServer {
            dev_tools_dir: Path::new(dev_tools_dir),
            bin_dir: Path::new(bin_dir),
        }),
        Box::new(TyposLsp {
            bin_dir: Path::new(bin_dir),
            sys_info: &sys_info,
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

    let installers_res = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(selected_installers.len());
        for installer in selected_installers {
            handles.push((installer.bin_name(), scope.spawn(move || installer.run())));
        }
        let mut res = Vec::with_capacity(handles.len());
        for (bin_name, handle) in handles {
            res.push((bin_name, handle.join()));
        }
        res
    });

    if let Err(errors) = report(&installers_res) {
        eprintln!(
            "{} | errors_count={} bin_names={errors:#?}",
            "error installing tools".red(),
            errors.len()
        );
        std::process::exit(1);
    }

    ytil_sys::rm::rm_dead_symlinks(bin_dir)?;

    Ok(())
}
