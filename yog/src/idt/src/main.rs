#![feature(exit_status_error)]

use std::path::Path;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

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
/// # Arguments
///
/// * `dev_tools_dir` - Directory for tool installation
/// * `bin_dir` - Directory for binary symlinks
/// * `tool_names` - Optional specific tools to install
///
/// # Examples
///
/// ```bash
/// idt ~/.dev-tools ~/.local/bin
/// idt ~/.dev-tools ~/.local/bin rust-analyzer typescript-language-server
/// ```
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = utils::system::get_args();
    println!("🚀 Starting {:#?} with args: {args:#?}", std::env::current_exe()?);

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

    utils::github::log_into_github()?;

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
                selected_installers.push(i)
            } else {
                unknown_installers.push(chosen_bin)
            }
        }
        (selected_installers, unknown_installers)
    };

    if !unknown_bin_names.is_empty() {
        eprintln!("⛔️ no installers matches the following bin names {unknown_bin_names:#?}");
    }

    let installers_errors = std::thread::scope(|scope| {
        let installers_handles = selected_installers
            .iter()
            .map(|installer| (installer.bin_name(), scope.spawn(move || installer.run())))
            .collect::<Vec<_>>();

        installers_handles
            .into_iter()
            .fold(vec![], |mut acc, (bin_name, handle)| {
                if let Err(error) = handle.join() {
                    eprintln!("❌ {bin_name} installer 🧵 panicked - error {error:#?}");
                    acc.push((bin_name, error));
                }
                acc
            })
    });

    utils::system::rm_dead_symlinks(bin_dir)?;

    let (errors_count, bin_names) = installers_errors.iter().fold((0, vec![]), |mut acc, (bin_name, _)| {
        acc.0 += 1;
        acc.1.push(bin_name);
        acc
    });

    if errors_count != 0 {
        // This is a general report about the installation process.
        // The single installation errors are reported directly in each [Installer].
        bail!("❌ {errors_count} bins failed to install, namely: {bin_names:#?}",)
    }

    Ok(())
}
