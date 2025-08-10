#![feature(exit_status_error)]

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
use crate::installers::hadonlint::Hadolint;
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

/// Install "Dev Tools"
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = utils::system::get_args();
    println!(
        "🚀 Starting {:#?} with args: {args:#?}",
        std::env::current_exe()?
    );

    let dev_tools_dir = args
        .first()
        .ok_or_else(|| eyre!("missing dev_tools_dir arg from {args:#?}"))?
        .trim_end_matches('/');
    let bin_dir = args
        .get(1)
        .ok_or_else(|| eyre!("missing bin_dir arg from {args:#?}"))?
        .trim_end_matches('/');
    let installers_whitelist: Vec<&str> = args.iter().skip(2).map(AsRef::as_ref).collect();

    std::fs::create_dir_all(dev_tools_dir)?;
    std::fs::create_dir_all(bin_dir)?;

    utils::github::log_into_github()?;

    let installers: Vec<Box<dyn Installer>> = vec![
        Box::new(BashLanguageServer {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(Commitlint {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(Deno {
            bin_dir: bin_dir.into(),
        }),
        Box::new(DockerLangServer {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(ElixirLs {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(ElmLanguageServer {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(EslintD {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(GraphQlLsp {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(Hadolint {
            bin_dir: bin_dir.into(),
        }),
        Box::new(HelmLs {
            bin_dir: bin_dir.into(),
        }),
        Box::new(LuaLanguageServer {
            dev_tools_dir: dev_tools_dir.into(),
        }),
        Box::new(Marksman {
            bin_dir: bin_dir.into(),
        }),
        Box::new(Nvim {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(PrettierD {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(Quicktype {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(RuffLsp {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(RustAnalyzer {
            bin_dir: bin_dir.into(),
        }),
        Box::new(Shellcheck {
            bin_dir: bin_dir.into(),
        }),
        Box::new(Sqruff {
            bin_dir: bin_dir.into(),
        }),
        Box::new(SqlLanguageServer {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(Taplo {
            bin_dir: bin_dir.into(),
        }),
        Box::new(TerraformLs {
            bin_dir: bin_dir.into(),
        }),
        Box::new(TypescriptLanguageServer {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(TyposLsp {
            bin_dir: bin_dir.into(),
        }),
        Box::new(VsCodeLangServers {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(YamlLanguageServer {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
    ];

    let whitelisted_installers: Vec<_> = if installers_whitelist.is_empty() {
        installers.iter().collect()
    } else {
        installers
            .iter()
            .filter(|x| installers_whitelist.contains(&x.bin_name()))
            .collect()
    };

    let installers_errors = std::thread::scope(|scope| {
        let installers_handles = whitelisted_installers
            .iter()
            .map(|installer| {
                let handle = scope.spawn(move || {
                    let install_result = installer.install();
                    // Reporting is done here, rather than after via `installers_errors`, to report
                    // results as soon as possible.
                    installer.report_install(install_result)
                });
                (installer.bin_name(), handle)
            })
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
    utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    let (errors_count, bin_names) =
        installers_errors
            .iter()
            .fold((0, vec![]), |mut acc, (bin_name, _)| {
                acc.0 += 1;
                acc.1.push(bin_name);
                acc
            });

    if errors_count != 0 {
        // This is a general report about the installation process.
        // The single installation errors are reported directly via [`tools::report_install`].
        bail!("❌ {errors_count} bins failed to install, namely: {bin_names:#?}",)
    }

    Ok(())
}
