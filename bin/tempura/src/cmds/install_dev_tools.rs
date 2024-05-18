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
        Box::new(|| tools::report_install("nvim", tools::nvim::install(dev_tools_dir, bin_dir))),
        Box::new(|| tools::report_install("rust_analyzer", tools::rust_analyzer::install(bin_dir))),
        Box::new(|| tools::report_install("taplo", tools::taplo::install(bin_dir))),
        Box::new(|| tools::report_install("terraform_ls", tools::terraform_ls::install(bin_dir))),
        Box::new(|| tools::report_install("deno", tools::deno::install(bin_dir))),
        Box::new(|| tools::report_install("typos_lsp", tools::typos_lsp::install(bin_dir))),
        Box::new(|| tools::report_install("vale", tools::vale::install(bin_dir))),
        Box::new(|| tools::report_install("hadolint", tools::hadonlint::install(bin_dir))),
        Box::new(|| tools::report_install("helm_ls", tools::helm_ls::install(bin_dir))),
        Box::new(|| tools::report_install("marksman", tools::marksman::install(bin_dir))),
        Box::new(|| tools::report_install("shellcheck", tools::shellcheck::install(bin_dir))),
        Box::new(|| {
            tools::report_install(
                "elixir_ls",
                tools::elixir_ls::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| tools::report_install("lua_ls", tools::lua_ls::install(bin_dir))),
        Box::new(|| {
            tools::report_install("phpactor", tools::phpactor::install(dev_tools_dir, bin_dir))
        }),
        Box::new(|| {
            tools::report_install(
                "php_cs_fixer",
                tools::php_cs_fixer::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| tools::report_install("psalm", tools::psalm::install(dev_tools_dir, bin_dir))),
        Box::new(|| {
            tools::report_install(
                "commitlint",
                tools::commitlint::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "elm_language_server",
                tools::elm_language_server::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "bash-language-server",
                tools::bash_language_server::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "docker_langserver",
                tools::docker_langserver::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install("eslint_d", tools::eslint_d::install(dev_tools_dir, bin_dir))
        }),
        Box::new(|| {
            tools::report_install(
                "graphql_lsp",
                tools::graphql_lsp::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "prettierd",
                tools::prettierd::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "sql_language_server",
                tools::sql_language_server::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "vscode_langservers",
                tools::vscode_langservers::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "yaml_language_server",
                tools::yaml_language_server::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "typescript_language_server",
                tools::typescript_language_server::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install(
                "quicktype",
                tools::quicktype::install(dev_tools_dir, bin_dir),
            )
        }),
        Box::new(|| {
            tools::report_install("ruff_lsp", tools::ruff_lsp::install(dev_tools_dir, bin_dir))
        }),
        Box::new(|| {
            tools::report_install("sqlfluff", tools::sqlfluff::install(dev_tools_dir, bin_dir))
        }),
    ];

    std::thread::scope(|scope| {
        let mut install_results = vec![];
        for tool_installer in tools_installers {
            install_results.push(scope.spawn(tool_installer));
        }
        for install_result in install_results {
            if let Err(e) = install_result.join() {
                eprintln!("❌ installer 🧵panicked: {e:?}");
            }
        }
    });

    crate::utils::system::chmod_x(&format!("{bin_dir}/*"))?;

    Ok(())
}
