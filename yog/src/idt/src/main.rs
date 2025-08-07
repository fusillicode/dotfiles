#![feature(exit_status_error)]

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

use crate::tools::Installer;
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
use crate::tools::prettierd::PrettierD;
use crate::tools::quicktype::Quicktype;
use crate::tools::ruff_lsp::RuffLsp;
use crate::tools::rust_analyzer::RustAnalyzer;
use crate::tools::shellcheck::Shellcheck;
use crate::tools::sql_language_server::SqlLanguageServer;
use crate::tools::sqruff::Sqruff;
use crate::tools::taplo::Taplo;
use crate::tools::terraform_ls::TerraformLs;
use crate::tools::typescript_language_server::TypescriptLanguageServer;
use crate::tools::typos_lsp::TyposLsp;
use crate::tools::vale::Vale;
use crate::tools::vscode_langservers::VsCodeLangServers;
use crate::tools::yaml_language_server::YamlLanguageServer;

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
    ];

    let whitelisted_tools: Vec<_> = if tools_whitelist.is_empty() {
        tools.iter().collect()
    } else {
        tools
            .iter()
            .filter(|x| tools_whitelist.contains(&x.bin_name()))
            .collect()
    };

    let tools_errors = std::thread::scope(|scope| {
        let tools_handles = whitelisted_tools
            .iter()
            .map(|installer| {
                let tool = installer.bin_name();
                let handle = scope.spawn(move || {
                    // Reporting is done here instead afterwards using `errors` to receive results as soon
                    // as possible.
                    tools::report_install(tool, installer.install())
                });
                (tool, handle)
            })
            .collect::<Vec<_>>();

        tools_handles
            .into_iter()
            .fold(vec![], |mut acc, (tool, handle)| {
                if let Err(error) = handle.join() {
                    eprintln!("❌ {tool} installer 🧵 panicked - error {error:?}");
                    acc.push((tool, error));
                }
                acc
            })
    });

    utils::system::rm_dead_symlinks(bin_dir)?;
    utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    let (errors_count, tools) = tools_errors.iter().fold((0, vec![]), |mut acc, (tool, _)| {
        acc.0 += 1;
        acc.1.push(tool);
        acc
    });
    if errors_count != 0 {
        // This is a general report about the installation process.
        // The single installation errors are reported directly via [`tools::report_install`].
        bail!("❌ {errors_count} tools failed to install, namely: {tools:#?}",)
    }

    Ok(())
}
