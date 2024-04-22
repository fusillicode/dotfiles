use std::fmt::Debug;

use anyhow::anyhow;

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

    let tools_installers: Vec<Box<dyn Fn() -> anyhow::Result<()> + Send + Sync>> = vec![
        Box::new(|| tools::nvim::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::rust_analyzer::install(bin_dir)),
        Box::new(|| tools::taplo::install(bin_dir)),
        Box::new(|| tools::terraform_ls::install(bin_dir)),
        Box::new(|| tools::deno::install(bin_dir)),
        Box::new(|| tools::typos_lsp::install(bin_dir)),
        Box::new(|| tools::vale::install(bin_dir)),
        Box::new(|| tools::hadonlint::install(bin_dir)),
        Box::new(|| tools::helm_ls::install(bin_dir)),
        Box::new(|| tools::marksman::install(bin_dir)),
        Box::new(|| tools::shellcheck::install(bin_dir)),
        Box::new(|| tools::elixir_ls::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::lua_ls::install(bin_dir)),
        Box::new(|| tools::phpactor::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::php_cs_fixer::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::psalm::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::commitlint::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::elm_language_server::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::bash_language_server::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::docker_langserver::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::eslint_d::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::graphql_lsp::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::prettierd::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::sql_language_server::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::vscode_langservers::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::yaml_language_server::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::typescript_language_server::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::quicktype::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::ruff_lsp::install(dev_tools_dir, bin_dir)),
        Box::new(|| tools::sqlfluff::install(dev_tools_dir, bin_dir)),
    ];

    std::thread::scope(|scope| {
        let mut install_results = vec![];
        for tool_installer in tools_installers {
            install_results.push(scope.spawn(tool_installer));
        }
        for install_result in install_results {
            if let Err(e) = install_result.join() {
                eprint!("failed to install tool: {e:?}");
            }
        }
    });

    crate::utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    Ok(())
}
