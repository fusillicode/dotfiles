use std::path::Path;
use std::path::PathBuf;

use chrono::Local;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;

const FILES_DIR: &[&str] = &["yog", "nvrim", "src", "attempt"];

pub fn dict() -> Dictionary {
    dict! {
        "select": fn_from!(select),
    }
}

fn select(_: ()) {
    let Ok(files_dir) = ytil_system::get_workspace_root()
        .map(|workspace_root| ytil_system::build_path(workspace_root, FILES_DIR))
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(&format!("cannot get workspace root | error={error:#?}"));
        })
        .and_then(|dir| std::fs::read_dir(dir).map_err(From::from))
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(&format!("cannot read attempt files dir | error={error:#?}"));
        })
    else {
        return;
    };

    let mut opts = vec![];
    for entry_res in files_dir {
        let Ok(path) = entry_res.map(|entry| entry.path()).inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(&format!("cannot get path of DirEntry | error={error:#?}"));
        }) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        let Ok(opt) = Opt::try_from(path).inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(&format!("{error}"));
        }) else {
            continue;
        };
        opts.push(opt);
    }

    let target_dir = Path::new("/tmp").join("attempt.rs");

    if let Err(error) = std::fs::create_dir_all(&target_dir) {
        ytil_nvim_oxi::api::notify_error(&format!(
            "cannot create target dir | target={:?} error={error:#?}",
            target_dir.display().to_string()
        ));
        return;
    }

    if let Err(error) = ytil_nvim_oxi::api::vim_ui_select(
        opts.iter().map(|opt| opt.display_name.clone()),
        &[("prompt", "Select file type ")],
        {
            let opts = opts.clone();
            move |choice_idx| {
                let Some(opt) = opts.get(choice_idx) else {
                    return;
                };
                let to = opt.target_file_path(&target_dir);
                if let Err(error) = std::fs::copy(opt.file_path.clone(), &to) {
                    ytil_nvim_oxi::api::notify_error(&format!(
                        "cannot copy file | from={:?} to={} error={error:#?}",
                        opt.file_path,
                        to.display()
                    ));
                    return;
                }
                if let Err(error) = nvim_oxi::api::command(&format!("edit {}", to.display())) {
                    ytil_nvim_oxi::api::notify_error(&format!(
                        "cannot open file in new buffer | path={} error={error:#?}",
                        to.display()
                    ));
                }
            }
        },
    ) {
        ytil_nvim_oxi::api::notify_error(&format!("{error}"));
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct Opt {
    display_name: String,
    base_name: String,
    extension: String,
    file_path: PathBuf,
}

impl Opt {
    pub fn target_file_path(&self, target: &Path) -> PathBuf {
        target.join(format!(
            "{}_{}.{}",
            self.base_name,
            Local::now().format("%Y%m%d_%H%M"),
            self.extension
        ))
    }
}

impl TryFrom<PathBuf> for Opt {
    type Error = color_eyre::eyre::Error;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let display_name = path
            .file_name()
            .map(|s| s.to_string_lossy())
            .ok_or_else(|| eyre!("missing file_name in path | path={path:?}"))?
            .to_string();
        let base_name = path
            .file_stem()
            .map(|s| s.to_string_lossy())
            .ok_or_else(|| eyre!("missing file_stem in path | path={path:?}"))?
            .to_string();
        let extension = path
            .extension()
            .map(|s| s.to_string_lossy())
            .ok_or_else(|| eyre!("missing extension in path | path={path:?}"))?
            .to_string();

        Ok(Self {
            display_name,
            base_name,
            extension,
            file_path: path,
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(PathBuf::from("/tmp/test.txt"), "test.txt", "test", "txt")]
    #[case(PathBuf::from("/tmp/.hidden.txt"), ".hidden.txt", ".hidden", "txt")]
    fn opt_try_from_path_buf_when_valid_file_returns_opt(
        #[case] path: PathBuf,
        #[case] expected_display: &str,
        #[case] expected_base: &str,
        #[case] expected_ext: &str,
    ) {
        assert2::let_assert!(Ok(actual) = Opt::try_from(path.clone()));
        pretty_assertions::assert_eq!(
            actual,
            Opt {
                display_name: expected_display.to_string(),
                base_name: expected_base.to_string(),
                extension: expected_ext.to_string(),
                file_path: path,
            },
        );
    }

    #[rstest]
    #[case(PathBuf::from("/tmp/test"), "missing extension")]
    #[case(PathBuf::from("/tmp/.hidden"), "missing extension")]
    #[case(PathBuf::from("/"), "missing file_name")]
    fn opt_try_from_path_buf_when_invalid_file_returns_error(#[case] path: PathBuf, #[case] expected_error: &str) {
        assert2::let_assert!(Err(error) = Opt::try_from(path));
        assert!(error.to_string().contains(expected_error));
    }

    #[test]
    fn opt_target_file_path_returns_correct_path() {
        let opt = Opt {
            display_name: "test.txt".to_string(),
            base_name: "test".to_string(),
            extension: "txt".to_string(),
            file_path: PathBuf::from("/some/path/test.txt"),
        };
        let result_path = opt.target_file_path(Path::new("/tmp"));
        let string_path = result_path.to_string_lossy();
        assert!(string_path.contains("/tmp/test_"));
        assert!(string_path.ends_with(".txt"));
    }
}
