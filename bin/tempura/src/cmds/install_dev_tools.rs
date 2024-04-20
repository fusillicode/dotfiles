use std::fmt::Debug;

use anyhow::anyhow;

mod composer_install;
mod curl_install;
mod npm_install;
mod pip_install;
mod tools;

// TODO: maybe this is enough to abstract installers 🤔
// type Installer = Box<dyn Fn(&str, &str) -> anyhow::Result<()>>;

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

    tools::nvim::install(dev_tools_dir, bin_dir)?;
    tools::rust_analyzer::install(bin_dir)?;
    tools::taplo::install(bin_dir)?;
    tools::terraform_ls::install(bin_dir)?;
    tools::deno::install(bin_dir)?;
    tools::typos_lsp::install(bin_dir)?;
    tools::vale::install(bin_dir)?;
    tools::hadonlint::install(bin_dir)?;
    tools::helm_ls::install(bin_dir)?;
    tools::marksman::install(bin_dir)?;
    tools::shellcheck::install(bin_dir)?;
    tools::elixir_ls::install(dev_tools_dir, bin_dir)?;
    tools::lua_ls::install(bin_dir)?;
    tools::phpactor::install(dev_tools_dir, bin_dir)?;
    tools::php_cs_fixer::install(dev_tools_dir, bin_dir)?;
    tools::psalm::install(dev_tools_dir, bin_dir)?;
    tools::commitlint::install(dev_tools_dir, bin_dir)?;
    tools::elm_language_server::install(dev_tools_dir, bin_dir)?;
    tools::bash_language_server::install(dev_tools_dir, bin_dir)?;
    tools::docker_langserver::install(dev_tools_dir, bin_dir)?;
    tools::eslint_d::install(dev_tools_dir, bin_dir)?;
    tools::graphql_lsp::install(dev_tools_dir, bin_dir)?;
    tools::prettierd::install(dev_tools_dir, bin_dir)?;
    tools::sql_language_server::install(dev_tools_dir, bin_dir)?;
    tools::vscode_langservers::install(dev_tools_dir, bin_dir)?;
    tools::yaml_language_server::install(dev_tools_dir, bin_dir)?;
    tools::typescript_language_server::install(dev_tools_dir, bin_dir)?;
    tools::quicktype::install(dev_tools_dir, bin_dir)?;
    tools::ruff_lsp::install(dev_tools_dir, bin_dir)?;
    tools::sqlfluff::install(dev_tools_dir, bin_dir)?;

    crate::utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    Ok(())
}
