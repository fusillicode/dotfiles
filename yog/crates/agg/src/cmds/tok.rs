use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_sys::pico_args::Arguments;

const DEFAULT_ENCODING: &str = "o200k_base";

#[derive(Debug, Eq, PartialEq)]
pub struct Opts {
    pub encoding: String,
    pub input: Input,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Input {
    File(PathBuf),
    Stdin,
    Text(String),
}

impl TryFrom<Vec<OsString>> for Opts {
    type Error = rootcause::Report;

    fn try_from(raw: Vec<OsString>) -> Result<Self, Self::Error> {
        let mut args = Arguments::from_vec(raw);
        let encoding = args
            .opt_value_from_str::<_, String>("--encoding")?
            .unwrap_or_else(|| DEFAULT_ENCODING.to_owned());
        let text = args.opt_value_from_str::<_, String>("--text")?;
        let positionals = args.finish();

        if let Some(argument) = positionals
            .iter()
            .find(|argument| argument.to_string_lossy().starts_with('-') && argument.as_os_str() != "-")
        {
            return Err(report!("unknown agg tok option").attach(format!("option={}", argument.to_string_lossy())));
        }

        let input = match (text, positionals.as_slice()) {
            (Some(text), []) => Input::Text(text),
            (Some(_), _) => return Err(report!("agg tok cannot combine --text with a file or stdin")),
            (None, [path]) if path == "-" => Input::Stdin,
            (None, [path]) => Input::File(PathBuf::from(path)),
            (None, []) => return Err(report!("agg tok requires a file, --text value, or - for stdin")),
            (None, _) => return Err(report!("agg tok accepts exactly one file or stdin input")),
        };

        Ok(Self { encoding, input })
    }
}

impl Input {
    fn read_to_string(&self) -> rootcause::Result<String> {
        match self {
            Self::File(path) => Ok(std::fs::read_to_string(path)
                .attach_with(|| format!("cannot read token input file | path={}", path.display()))?),
            Self::Stdin => {
                let mut text = String::new();
                std::io::stdin()
                    .read_to_string(&mut text)
                    .context("cannot read token input from stdin")?;
                Ok(text)
            }
            Self::Text(text) => Ok(text.clone()),
        }
    }
}

pub fn run(options: &Opts) -> rootcause::Result<()> {
    let encoding = tiktoken::get_encoding(&options.encoding).ok_or_else(|| {
        report!("unknown token encoding")
            .attach(format!("encoding={}", options.encoding))
            .attach(format!("supported={}", tiktoken::list_encodings().join(", ")))
    })?;
    let text = options.input.read_to_string()?;
    let tokens = encoding.count(&text);

    println!("{tokens}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_input_when_text_returns_the_text() {
        let input = Input::Text("hello world".to_owned());

        assert_that!(input.read_to_string(), ok(eq("hello world")));
    }

    #[test]
    fn test_input_when_file_reads_utf8_contents() {
        let directory = tempfile::tempdir().expect("test directory should be created");
        let path = directory.path().join("prompt.txt");
        std::fs::write(&path, "hello world").expect("test input should be written");

        assert_that!(Input::File(path).read_to_string(), ok(eq("hello world")));
    }

    #[test]
    fn test_input_when_file_is_missing_reports_the_path() {
        let path = Path::new("missing-prompt.txt");

        assert_that!(
            (Input::File(path.to_owned()).read_to_string()).map(|_| ()),
            err(displays_as(all!(
                contains_substring("cannot read token input file"),
                contains_substring(path.to_str().expect("test path should be valid UTF-8"))
            )))
        );
    }

    #[test]
    fn test_count_with_default_encoding_counts_known_text() {
        let encoding = tiktoken::get_encoding(DEFAULT_ENCODING).expect("default encoding should be available");

        assert_eq!(encoding.count("hello world"), 2);
    }

    #[test]
    fn test_run_when_encoding_is_unknown_reports_supported_encodings() {
        let options = Opts {
            encoding: "unknown".to_owned(),
            input: Input::Text("hello".to_owned()),
        };

        assert_that!(
            run(&options),
            err(displays_as(all!(
                contains_substring("unknown token encoding"),
                contains_substring("encoding=unknown"),
                contains_substring(DEFAULT_ENCODING)
            )))
        );
    }

    #[rstest::rstest]
    #[case::missing_input(&[])]
    #[case::multiple_files(&["first.txt", "second.txt"])]
    #[case::text_and_file(&["--text", "hello", "prompt.txt"])]
    #[case::unknown_option(&["--unknown", "prompt.txt"])]
    fn test_options_when_input_arguments_are_invalid_rejects_command(#[case] args: &[&str]) {
        assert_that!(parse(args), err(anything()));
    }

    fn parse(args: &[&str]) -> rootcause::Result<Opts> {
        Opts::try_from(args.iter().map(OsString::from).collect::<Vec<_>>())
    }
}
