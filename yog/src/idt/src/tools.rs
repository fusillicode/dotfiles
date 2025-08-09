pub mod bash_language_server;
pub mod commitlint;
pub mod deno;
pub mod docker_langserver;
pub mod elixir_ls;
pub mod elm_language_server;
pub mod eslint_d;
pub mod graphql_lsp;
pub mod hadonlint;
pub mod helm_ls;
pub mod lua_ls;
pub mod marksman;
pub mod nvim;
pub mod prettierd;
pub mod quicktype;
pub mod ruff_lsp;
pub mod rust_analyzer;
pub mod shellcheck;
pub mod sql_language_server;
pub mod sqruff;
pub mod taplo;
pub mod terraform_ls;
pub mod typescript_language_server;
pub mod typos_lsp;
pub mod vscode_langservers;
pub mod yaml_language_server;

pub trait ToolInstaller: Sync + Send {
    fn bin_name(&self) -> &'static str;

    fn download(&self) -> color_eyre::Result<()>;

    fn install(&self) -> color_eyre::Result<()> {
        self.download()?;
        Ok(())
    }

    fn report_install_res(&self, install_res: color_eyre::Result<()>) -> color_eyre::Result<()> {
        install_res
            .inspect(|_| println!("🎉 {} installed", self.bin_name()))
            .inspect_err(|e| eprintln!("❌ error installing {}: {e:#?}", self.bin_name()))
    }
}
