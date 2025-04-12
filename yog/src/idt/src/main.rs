#![feature(exit_status_error)]

use color_eyre::eyre::eyre;

use crate::tools::bash_language_server::BashLanguageServer;
use crate::tools::commitlint::Commitlint;
use crate::tools::deno::Deno;
use crate::tools::docker_langserver::DockerLangServer;
use crate::tools::elixir_ls::ElixirLs;
use crate::tools::elm_language_server::ElmLanguageServer;
use crate::tools::eslint_d::EslintD;
use crate::tools::graphql_lsp::GraphQlLsp;
use crate::tools::hadonlint::Hadolint;
use crate::tools::helm_ls::HelmLs;
use crate::tools::lua_ls::LuaLanguageServer;
use crate::tools::marksman::Marksman;
use crate::tools::nvim::Nvim;
use crate::tools::ollama::Ollama;
use crate::tools::prettierd::PrettierD;
use crate::tools::quicktype::Quicktype;
use crate::tools::ruff_lsp::RuffLsp;
use crate::tools::rust_analyzer::RustAnalyzer;
use crate::tools::shellcheck::Shellcheck;
use crate::tools::sql_language_server::SqlLanguageServer;
use crate::tools::sqlfluff::SqlFluff;
use crate::tools::taplo::Taplo;
use crate::tools::terraform_ls::TerraformLs;
use crate::tools::typescript_language_server::TypescriptLanguageServer;
use crate::tools::typos_lsp::TyposLsp;
use crate::tools::vale::Vale;
use crate::tools::vscode_langservers::VsCodeLangServers;
use crate::tools::yaml_language_server::YamlLanguageServer;
use crate::tools::Installer;

mod installers;
mod tools;

/// Install "Dev Tools"
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = utils::system::get_args();
    println!(
        "🚀 Starting {:?} with args: {args:#?}",
        std::env::current_exe()?
    );

    let dev_tools_dir = args
        .first()
        .ok_or_else(|| eyre!("missing dev_tools_dir arg from {args:?}"))?
        .trim_end_matches('/');
    let bin_dir = args
        .get(1)
        .ok_or_else(|| eyre!("missing bin_dir arg from {args:?}"))?
        .trim_end_matches('/');
    let tools_whitelist: Vec<&str> = args.iter().skip(2).map(AsRef::as_ref).collect();

    std::fs::create_dir_all(dev_tools_dir)?;
    std::fs::create_dir_all(bin_dir)?;

    utils::github::log_into_github()?;

    let tools: Vec<Box<dyn Installer>> = vec![
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
        Box::new(SqlFluff {
            dev_tools_dir: dev_tools_dir.into(),
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
        Box::new(Vale {
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
        Box::new(Ollama {
            bin_dir: bin_dir.into(),
        }),
    ];

    let whitelisted_tools: Vec<_> = if tools_whitelist.is_empty() {
        tools.iter().collect()
    } else {
        tools
            .iter()
            .filter(|x| tools_whitelist.contains(&x.bin_name()))
            .collect()
    };

    std::thread::scope(|scope| {
        whitelisted_tools
            .iter()
            .fold(vec![], |mut acc, installer| {
                let running_installer = scope.spawn(move || {
                    tools::report_install(installer.bin_name(), installer.install())
                });
                acc.push((installer.bin_name(), running_installer));
                acc
            })
            .into_iter()
            .for_each(|(tool, running_installer)| {
                if let Err(error) = running_installer.join() {
                    eprintln!("❌ {tool} installer 🧵 panicked: {error:?}");
                }
            });
    });

    utils::system::rm_dead_symlinks(bin_dir)?;
    utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    Ok(())
}
