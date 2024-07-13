use std::fmt::Debug;

use anyhow::anyhow;

use crate::cmds::install_dev_tools::tools::bash_language_server::BashLanguageServerInstaller;
use crate::cmds::install_dev_tools::tools::commitlint::CommitlintInstaller;
use crate::cmds::install_dev_tools::tools::deno::DenoInstaller;
use crate::cmds::install_dev_tools::tools::docker_langserver::DockerLangServerInstaller;
use crate::cmds::install_dev_tools::tools::elixir_ls::ElixirLsInstaller;
use crate::cmds::install_dev_tools::tools::elm_language_server::ElmLanguageServerInstaller;
use crate::cmds::install_dev_tools::tools::eslint_d::EslintDInstaller;
use crate::cmds::install_dev_tools::tools::graphql_lsp::GraphQlLspInstaller;
use crate::cmds::install_dev_tools::tools::hadonlint::HadolintInstaller;
use crate::cmds::install_dev_tools::tools::helm_ls::HelmLsInstaller;
use crate::cmds::install_dev_tools::tools::lua_ls::LuaLanguageServer;
use crate::cmds::install_dev_tools::tools::marksman::MarksmanInstaller;
use crate::cmds::install_dev_tools::tools::nvim::NvimInstaller;
use crate::cmds::install_dev_tools::tools::ollama::OllamaInstaller;
use crate::cmds::install_dev_tools::tools::php_cs_fixer::PhpFixerInstaller;
use crate::cmds::install_dev_tools::tools::phpactor::PhpActorInstaller;
use crate::cmds::install_dev_tools::tools::prettierd::PrettierDInstaller;
use crate::cmds::install_dev_tools::tools::psalm::PsalmInstaller;
use crate::cmds::install_dev_tools::tools::quicktype::QuicktypeInstaller;
use crate::cmds::install_dev_tools::tools::ruff_lsp::RuffLspInstaller;
use crate::cmds::install_dev_tools::tools::rust_analyzer::RustAnalyzerInstaller;
use crate::cmds::install_dev_tools::tools::shellcheck::ShellcheckInstaller;
use crate::cmds::install_dev_tools::tools::sql_language_server::SqlLanguageServerInstaller;
use crate::cmds::install_dev_tools::tools::sqlfluff::SqlFluffInstaller;
use crate::cmds::install_dev_tools::tools::taplo::TaploInstaller;
use crate::cmds::install_dev_tools::tools::terraform_ls::TerraformLsInstaller;
use crate::cmds::install_dev_tools::tools::typescript_language_server::TypescriptLanguageServerInstaller;
use crate::cmds::install_dev_tools::tools::typos_lsp::TyposLspInstaller;
use crate::cmds::install_dev_tools::tools::vale::ValeInstaller;
use crate::cmds::install_dev_tools::tools::vscode_langservers::VsCodeLangServersInstaller;
use crate::cmds::install_dev_tools::tools::yaml_language_server::YamlLanguageServerInstaller;
use crate::cmds::install_dev_tools::tools::Installer;

mod composer_install;
mod curl_install;
mod npm_install;
mod pip_install;
mod tools;

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

    crate::utils::github::log_into_github()?;

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

    crate::utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    Ok(())
}
