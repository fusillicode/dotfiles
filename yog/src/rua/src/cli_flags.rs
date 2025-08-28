pub mod fd;
pub mod rg;

pub const GLOB_BLACKLIST: [&str; 6] = [
    "**/.git/*",
    "**/target/*",
    "**/_build/*",
    "**/deps/*",
    "**/.elixir_ls/*",
    "**/node_modules/*",
];

pub trait CliFlags {
    fn base_flags() -> Vec<&'static str>;

    fn glob_flag(glob: &str) -> String;

    fn get(&self) -> Box<dyn Fn(()) -> Vec<String>> {
        Box::new(|_| {
            Self::base_flags()
                .into_iter()
                .map(Into::into)
                .chain(GLOB_BLACKLIST.into_iter().map(Self::glob_flag))
                .collect::<Vec<_>>()
        })
    }
}
