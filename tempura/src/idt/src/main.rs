#![feature(exit_status_error)]
use anyhow::anyhow;

use crate::tools::bash_language_server::BashLanguageServerInstaller;
use crate::tools::commitlint::CommitlintInstaller;
use crate::tools::deno::DenoInstaller;
use crate::tools::docker_langserver::DockerLangServerInstaller;
use crate::tools::elixir_ls::ElixirLsInstaller;
use crate::tools::elm_language_server::ElmLanguageServerInstaller;
use crate::tools::eslint_d::EslintDInstaller;
use crate::tools::graphql_lsp::GraphQlLspInstaller;
use crate::tools::hadonlint::HadolintInstaller;
use crate::tools::helm_ls::HelmLsInstaller;
use crate::tools::lua_ls::LuaLanguageServer;
use crate::tools::marksman::MarksmanInstaller;
use crate::tools::nvim::NvimInstaller;
use crate::tools::ollama::OllamaInstaller;
use crate::tools::php_cs_fixer::PhpFixerInstaller;
use crate::tools::phpactor::PhpActorInstaller;
use crate::tools::prettierd::PrettierDInstaller;
use crate::tools::psalm::PsalmInstaller;
use crate::tools::quicktype::QuicktypeInstaller;
use crate::tools::ruff_lsp::RuffLspInstaller;
use crate::tools::rust_analyzer::RustAnalyzerInstaller;
use crate::tools::shellcheck::ShellcheckInstaller;
use crate::tools::sql_language_server::SqlLanguageServerInstaller;
use crate::tools::sqlfluff::SqlFluffInstaller;
use crate::tools::taplo::TaploInstaller;
use crate::tools::terraform_ls::TerraformLsInstaller;
use crate::tools::typescript_language_server::TypescriptLanguageServerInstaller;
use crate::tools::typos_lsp::TyposLspInstaller;
use crate::tools::vale::ValeInstaller;
use crate::tools::vscode_langservers::VsCodeLangServersInstaller;
use crate::tools::yaml_language_server::YamlLanguageServerInstaller;
use crate::tools::Installer;

mod installers;
mod tools;

/// Install Dev Tools
fn main() -> anyhow::Result<()> {
    let args = utils::get_args();

    let dev_tools_dir = args
        .first()
        .ok_or_else(|| anyhow!("missing dev_tools_dir arg from {args:?}"))?
        .trim_end_matches('/');
    let bin_dir = args
        .get(1)
        .ok_or_else(|| anyhow!("missing bin_dir arg from {args:?}"))?
        .trim_end_matches('/');

    std::fs::create_dir_all(dev_tools_dir)?;
    std::fs::create_dir_all(bin_dir)?;

    utils::github::log_into_github()?;

    let installers: Vec<Box<dyn Installer>> = vec![
        Box::new(BashLanguageServerInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(CommitlintInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(DenoInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(DockerLangServerInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(ElixirLsInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(ElmLanguageServerInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(EslintDInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(GraphQlLspInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(HadolintInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(HelmLsInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(LuaLanguageServer {
            dev_tools_dir: dev_tools_dir.into(),
        }),
        Box::new(MarksmanInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(NvimInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(PhpActorInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(PhpFixerInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(PrettierDInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(PsalmInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(QuicktypeInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(RuffLspInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(RustAnalyzerInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(ShellcheckInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(SqlFluffInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(SqlLanguageServerInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(TaploInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(TerraformLsInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(TypescriptLanguageServerInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(TyposLspInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(ValeInstaller {
            bin_dir: bin_dir.into(),
        }),
        Box::new(VsCodeLangServersInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(YamlLanguageServerInstaller {
            dev_tools_dir: dev_tools_dir.into(),
            bin_dir: bin_dir.into(),
        }),
        Box::new(OllamaInstaller {
            bin_dir: bin_dir.into(),
        }),
    ];

    std::thread::scope(|scope| {
        installers
            .iter()
            .fold(vec![], |mut acc, installer| {
                let running_installer = scope
                    .spawn(move || tools::report_install(installer.bin(), installer.install()));
                acc.push((installer.bin(), running_installer));
                acc
            })
            .into_iter()
            .for_each(|(tool, running_installer)| {
                if let Err(e) = running_installer.join() {
                    eprintln!("❌ {tool} installer 🧵 panicked: {e:?}");
                }
            });
    });

    utils::system::rm_dead_symlinks(bin_dir)?;
    utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    Ok(())
}
