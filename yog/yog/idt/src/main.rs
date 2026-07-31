//! Install language servers, linters, formatters, and developer helpers concurrently.
//!
//! # Errors
//! - Missing required argument (`dev_tools_dir` / `bin_dir`).
//! - Directory creation fails.
//! - GitHub authentication fails.
//! - Installer thread panics.
//! - Individual tool installation fails.
//! - Dead symlink cleanup fails.
#![feature(exit_status_error)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_sys::SysInfo;
use ytil_sys::cli::Args;

use crate::installers::Installer;
use crate::installers::alacritty::Alacritty;
use crate::installers::bash_language_server::BashLanguageServer;
use crate::installers::cargo::Cargo;
use crate::installers::cargo::cargo_bin_dir;
use crate::installers::commitlint::Commitlint;
use crate::installers::deno::Deno;
use crate::installers::docker_langserver::DockerLangServer;
use crate::installers::eslint_d::EslintD;
use crate::installers::graphql_lsp::GraphQlLsp;
use crate::installers::hadolint::Hadolint;
use crate::installers::helm_ls::HelmLs;
use crate::installers::lua_ls::LuaLanguageServer;
use crate::installers::marksman::Marksman;
use crate::installers::nvim::Nvim;
use crate::installers::opencode::Opencode;
use crate::installers::prettierd::PrettierD;
use crate::installers::quicktype::Quicktype;
use crate::installers::rio::Rio;
use crate::installers::ruff_lsp::RuffLsp;
use crate::installers::shellcheck::Shellcheck;
use crate::installers::sql_language_server::SqlLanguageServer;
use crate::installers::sqruff::Sqruff;
use crate::installers::starship::Starship;
use crate::installers::terraform_ls::TerraformLs;
use crate::installers::typescript_language_server::TypescriptLanguageServer;
use crate::installers::typos_lsp::TyposLsp;
use crate::installers::vscode_langservers::VsCodeLangServers;
use crate::installers::yaml_language_server::YamlLanguageServer;
use crate::installers::zellij::Zellij;

mod downloaders;
mod installers;

/// Install language servers, linters, formatters, and developer helpers concurrently.
#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
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
        .ok_or_else(|| report!("missing dev_tools_dir arg"))
        .attach_with(|| format!("args={args:#?}"))?
        .trim_end_matches('/');
    let bin_dir = args
        .get(1)
        .ok_or_else(|| report!("missing bin_dir arg"))
        .attach_with(|| format!("args={args:#?}"))?
        .trim_end_matches('/');
    let supplied_bin_names: Vec<&str> = args.iter().skip(2).map(AsRef::as_ref).collect();

    let sys_info = SysInfo::get()?;

    let dev_tools_path = Path::new(dev_tools_dir);
    let bin_path = Path::new(bin_dir);
    std::fs::create_dir_all(dev_tools_path)?;
    std::fs::create_dir_all(bin_path)?;

    let cargo_bin_dir = cargo_bin_dir()?;
    let parallel_installers = parallel_installers(dev_tools_path, bin_path, &sys_info);
    let managed_cargo_installers = managed_cargo_installers(&cargo_bin_dir, bin_path);
    let (selected_parallel_installers, selected_cargo_installers, unknown_bin_names) =
        select_installers(&supplied_bin_names, &parallel_installers, &managed_cargo_installers);

    if !unknown_bin_names.is_empty() {
        eprintln!(
            "{} bins without matching installers",
            format!("{unknown_bin_names:#?}").yellow().bold()
        );
    }

    let installers_res = run_installers(selected_parallel_installers, selected_cargo_installers);

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

/// Construct installers that can run independently.
fn parallel_installers<'a>(
    dev_tools_dir: &'a Path,
    bin_dir: &'a Path,
    sys_info: &'a SysInfo,
) -> Vec<Box<dyn Installer + 'a>> {
    vec![
        Box::new(Alacritty { dev_tools_dir, bin_dir }),
        Box::new(BashLanguageServer { dev_tools_dir, bin_dir }),
        Box::new(Commitlint { dev_tools_dir, bin_dir }),
        Box::new(Deno { bin_dir, sys_info }),
        Box::new(DockerLangServer { dev_tools_dir, bin_dir }),
        Box::new(EslintD { dev_tools_dir, bin_dir }),
        Box::new(GraphQlLsp { dev_tools_dir, bin_dir }),
        Box::new(Hadolint { bin_dir, sys_info }),
        Box::new(HelmLs { bin_dir, sys_info }),
        Box::new(LuaLanguageServer {
            dev_tools_dir,
            sys_info,
        }),
        Box::new(Marksman { bin_dir, sys_info }),
        Box::new(Nvim { dev_tools_dir, bin_dir }),
        Box::new(Opencode { bin_dir, sys_info }),
        Box::new(PrettierD { dev_tools_dir, bin_dir }),
        Box::new(Quicktype { dev_tools_dir, bin_dir }),
        Box::new(Rio { dev_tools_dir, bin_dir }),
        Box::new(RuffLsp { dev_tools_dir, bin_dir }),
        Box::new(Shellcheck { bin_dir, sys_info }),
        Box::new(Sqruff { bin_dir, sys_info }),
        Box::new(SqlLanguageServer { dev_tools_dir, bin_dir }),
        Box::new(Starship { dev_tools_dir, bin_dir }),
        Box::new(TerraformLs { bin_dir, sys_info }),
        Box::new(TypescriptLanguageServer { dev_tools_dir, bin_dir }),
        Box::new(TyposLsp { bin_dir, sys_info }),
        Box::new(VsCodeLangServers { dev_tools_dir, bin_dir }),
        Box::new(YamlLanguageServer { dev_tools_dir, bin_dir }),
        Box::new(Zellij { dev_tools_dir, bin_dir }),
    ]
}

/// Construct the declared Cargo and Git tool inventory.
fn managed_cargo_installers<'a>(cargo_bin_dir: &'a Path, bin_dir: &'a Path) -> Vec<Box<dyn Installer + 'a>> {
    vec![
        Box::new(Cargo::registry(
            cargo_bin_dir,
            bin_dir,
            "cargo-auditable",
            "cargo-auditable",
        )),
        Box::new(Cargo::registry_with_features(
            cargo_bin_dir,
            bin_dir,
            "cargo-audit",
            "cargo-audit",
            "fix",
        )),
        Box::new(Cargo::registry(
            cargo_bin_dir,
            bin_dir,
            "cargo-machete",
            "cargo-machete",
        )),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "cargo-make", "cargo-make")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "cargo-sort", "cargo-sort")),
        Box::new(Cargo::registry(
            cargo_bin_dir,
            bin_dir,
            "cargo-sort-derives",
            "cargo-sort-derives",
        )),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "ccase", "ccase")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "fd", "fd-find")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "jnv", "jnv")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "mise", "mise")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "pv", "pv")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "qj", "qj")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "rg", "ripgrep")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "sd", "sd")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "sqlx", "sqlx-cli")),
        Box::new(Cargo::registry(
            cargo_bin_dir,
            bin_dir,
            "tree-sitter",
            "tree-sitter-cli",
        )),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "typos", "typos-cli")),
        Box::new(Cargo::registry(cargo_bin_dir, bin_dir, "harper-ls", "harper-ls")),
        Box::new(Cargo::registry_with_all_features(
            cargo_bin_dir,
            bin_dir,
            "taplo",
            "taplo-cli",
        )),
        Box::new(Cargo::nightly_git(
            cargo_bin_dir,
            bin_dir,
            "rust-analyzer",
            "https://github.com/rust-lang/rust-analyzer.git",
            "master",
        )),
        Box::new(Cargo::locked_registry(
            cargo_bin_dir,
            bin_dir,
            "cargo-nextest",
            "cargo-nextest",
        )),
        Box::new(Cargo::registry(
            cargo_bin_dir,
            bin_dir,
            "cargo-llvm-cov",
            "cargo-llvm-cov",
        )),
        Box::new(Cargo::git(
            cargo_bin_dir,
            bin_dir,
            "rtk",
            "https://github.com/rtk-ai/rtk",
        )),
    ]
}

/// Select individual installers or expand the `cargo` selector group.
fn select_installers<'installer, 'name>(
    supplied_bin_names: &[&'name str],
    parallel_installers: &'installer [Box<dyn Installer + 'installer>],
    managed_cargo_installers: &'installer [Box<dyn Installer + 'installer>],
) -> (
    Vec<&'installer dyn Installer>,
    Vec<&'installer dyn Installer>,
    Vec<&'name str>,
) {
    if supplied_bin_names.is_empty() {
        return (
            parallel_installers.iter().map(Box::as_ref).collect(),
            managed_cargo_installers.iter().map(Box::as_ref).collect(),
            vec![],
        );
    }

    let parallel_installer_map: HashMap<&str, &dyn Installer> = parallel_installers
        .iter()
        .map(|installer| (installer.bin_name(), installer.as_ref()))
        .collect();
    let cargo_installer_map: HashMap<&str, &dyn Installer> = managed_cargo_installers
        .iter()
        .map(|installer| (installer.bin_name(), installer.as_ref()))
        .collect();

    let mut selected_parallel_installers = Vec::with_capacity(supplied_bin_names.len());
    let mut selected_cargo_installers = Vec::with_capacity(supplied_bin_names.len());
    let mut unknown_installers = vec![];
    let mut selected_bin_names = HashSet::new();
    for chosen_bin in supplied_bin_names {
        if *chosen_bin == "cargo" {
            for installer in managed_cargo_installers {
                if selected_bin_names.insert(installer.bin_name()) {
                    selected_cargo_installers.push(installer.as_ref());
                }
            }
        } else if let Some(&installer) = parallel_installer_map.get(chosen_bin) {
            if selected_bin_names.insert(installer.bin_name()) {
                selected_parallel_installers.push(installer);
            }
        } else if let Some(&installer) = cargo_installer_map.get(chosen_bin) {
            if selected_bin_names.insert(installer.bin_name()) {
                selected_cargo_installers.push(installer);
            }
        } else {
            unknown_installers.push(*chosen_bin);
        }
    }

    (
        selected_parallel_installers,
        selected_cargo_installers,
        unknown_installers,
    )
}

/// Run independent installers concurrently and Cargo installers on one serial worker.
fn run_installers<'a>(
    selected_parallel_installers: Vec<&'a dyn Installer>,
    selected_cargo_installers: Vec<&'a dyn Installer>,
) -> Vec<(&'a str, std::thread::Result<rootcause::Result<()>>)> {
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(selected_parallel_installers.len());
        for installer in selected_parallel_installers {
            handles.push((installer.bin_name(), scope.spawn(move || installer.run())));
        }
        let cargo_handle = scope.spawn(move || {
            selected_cargo_installers
                .into_iter()
                .map(|installer| (installer.bin_name(), installer.run()))
                .collect::<Vec<_>>()
        });

        let mut results = Vec::with_capacity(handles.len());
        for (bin_name, handle) in handles {
            results.push((bin_name, handle.join()));
        }
        match cargo_handle.join() {
            Ok(cargo_results) => results.extend(
                cargo_results
                    .into_iter()
                    .map(|(bin_name, result)| (bin_name, Ok(result))),
            ),
            Err(error) => results.push(("cargo", Err(error))),
        }
        results
    })
}

/// Summarize installer thread outcomes; collect failing bin names.
///
/// # Errors
/// Returns failing bin names; installers handle detailed error output.
fn report<'a>(installers_res: &'a [(&'a str, std::thread::Result<rootcause::Result<()>>)]) -> Result<(), Vec<&'a str>> {
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
