use nvim_oxi::Function;
use nvim_oxi::Object;

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

    fn get(&self) -> Object {
        Object::from(Function::<(), _>::from_fn(|_| {
            Self::base_flags()
                .into_iter()
                .map(Into::into)
                .chain(GLOB_BLACKLIST.into_iter().map(Self::glob_flag))
                .collect::<Vec<_>>()
        }))
    }
}
