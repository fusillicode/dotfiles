use std::fmt::Debug;

use anyhow::anyhow;

use crate::cmds::idt::tools::bash_language_server::BashLanguageServerInstaller;
use crate::cmds::idt::tools::commitlint::CommitlintInstaller;
use crate::cmds::idt::tools::deno::DenoInstaller;
use crate::cmds::idt::tools::docker_langserver::DockerLangServerInstaller;
use crate::cmds::idt::tools::elixir_ls::ElixirLsInstaller;
use crate::cmds::idt::tools::elm_language_server::ElmLanguageServerInstaller;
use crate::cmds::idt::tools::eslint_d::EslintDInstaller;
use crate::cmds::idt::tools::graphql_lsp::GraphQlLspInstaller;
use crate::cmds::idt::tools::hadonlint::HadolintInstaller;
use crate::cmds::idt::tools::helm_ls::HelmLsInstaller;
use crate::cmds::idt::tools::lua_ls::LuaLanguageServer;
use crate::cmds::idt::tools::marksman::MarksmanInstaller;
use crate::cmds::idt::tools::nvim::NvimInstaller;
use crate::cmds::idt::tools::ollama::OllamaInstaller;
use crate::cmds::idt::tools::php_cs_fixer::PhpFixerInstaller;
use crate::cmds::idt::tools::phpactor::PhpActorInstaller;
use crate::cmds::idt::tools::prettierd::PrettierDInstaller;
use crate::cmds::idt::tools::psalm::PsalmInstaller;
use crate::cmds::idt::tools::quicktype::QuicktypeInstaller;
use crate::cmds::idt::tools::ruff_lsp::RuffLspInstaller;
use crate::cmds::idt::tools::rust_analyzer::RustAnalyzerInstaller;
use crate::cmds::idt::tools::shellcheck::ShellcheckInstaller;
use crate::cmds::idt::tools::sql_language_server::SqlLanguageServerInstaller;
use crate::cmds::idt::tools::sqlfluff::SqlFluffInstaller;
use crate::cmds::idt::tools::taplo::TaploInstaller;
use crate::cmds::idt::tools::terraform_ls::TerraformLsInstaller;
use crate::cmds::idt::tools::typescript_language_server::TypescriptLanguageServerInstaller;
use crate::cmds::idt::tools::typos_lsp::TyposLspInstaller;
use crate::cmds::idt::tools::vale::ValeInstaller;
use crate::cmds::idt::tools::vscode_langservers::VsCodeLangServersInstaller;
use crate::cmds::idt::tools::yaml_language_server::YamlLanguageServerInstaller;
use crate::cmds::idt::tools::Installer;

mod composer_install;
mod curl_install;
mod npm_install;
mod pip_install;
mod tools;

/// Install Dev Tools
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

    crate::utils::system::rm_dead_symlinks(bin_dir)?;
    crate::utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    Ok(())
}
