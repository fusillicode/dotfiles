use std::fs::DirEntry;
use std::fs::ReadDir;
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
    let Ok(files_dir) = get_attempt_files() else {
        return;
    };

    let mut opts = vec![];
    for entry_res in files_dir {
        let Some(opt_res) = Opt::build(entry_res) else {
            continue;
        };
        let Ok(opt) = opt_res.inspect_err(|error| {
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

fn get_attempt_files() -> color_eyre::Result<ReadDir> {
    ytil_system::get_workspace_root()
        .map(|workspace_root| ytil_system::build_path(workspace_root, FILES_DIR))
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(&format!("cannot get workspace root | error={error:#?}"));
        })
        .and_then(|dir| std::fs::read_dir(dir).map_err(From::from))
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(&format!("cannot read attempt files dir | error={error:#?}"));
        })
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
    pub fn build(read_dir: std::io::Result<DirEntry>) -> Option<color_eyre::Result<Self>> {
        let path = match read_dir.map(|entry| entry.path()) {
            Ok(path) => path,
            Err(e) => return Some(Err(e.into())),
        };
        if !path.is_file() {
            return None;
        }
        let display_name = match path.file_name().map(|s| s.to_string_lossy()) {
            Some(s) => s.to_string(),
            None => return Some(Err(eyre!("missing file_name in path | path={path:?}"))),
        };
        let base_name = match path.file_stem().map(|s| s.to_string_lossy()) {
            Some(s) => s.to_string(),
            None => return Some(Err(eyre!("missing file_stem in path | path={path:?}"))),
        };
        let extension = match path.extension().map(|s| s.to_string_lossy()) {
            Some(s) => s.to_string(),
            None => return Some(Err(eyre!("missing extension in path | path={path:?}"))),
        };

        Some(Ok(Self {
            display_name,
            base_name,
            extension,
            file_path: path,
        }))
    }

    pub fn target_file_path(&self, target: &Path) -> PathBuf {
        target.join(format!(
            "{}_{}.{}",
            self.base_name,
            Local::now().format("%Y%m%d_%H%M"),
            self.extension
        ))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    #[case("test.txt", "test.txt", "test", "txt")]
    #[case(".hidden.txt", ".hidden.txt", ".hidden", "txt")]
    fn opt_build_when_valid_file_returns_some_ok(
        #[case] file_name: &str,
        #[case] expected_display: &str,
        #[case] expected_base: &str,
        #[case] expected_ext: &str,
    ) {
        let (_tmp_dir, entry) = dummy_dir_entry(file_name);
        let expected_path = entry.path();

        let result = Opt::build(Ok(entry));

        assert2::let_assert!(Some(Ok(actual)) = result);
        pretty_assertions::assert_eq!(
            actual,
            Opt {
                display_name: expected_display.to_string(),
                base_name: expected_base.to_string(),
                extension: expected_ext.to_string(),
                file_path: expected_path,
            },
        );
    }

    #[test]
    fn opt_build_when_directory_returns_none() {
        let temp_dir = TempDir::new().unwrap();
        let sub_dir = temp_dir.path().join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        let mut read_dir = std::fs::read_dir(temp_dir.path()).unwrap();
        let entry = read_dir.next().unwrap().unwrap();

        let result = Opt::build(Ok(entry));

        assert!(result.is_none());
    }

    #[rstest]
    #[case("test", "missing extension")]
    #[case(".hidden", "missing extension")]
    fn opt_build_when_invalid_file_returns_some_expected_error(#[case] file_name: &str, #[case] expected_error: &str) {
        let (_tmp_dir, entry) = dummy_dir_entry(file_name);

        let result = Opt::build(Ok(entry));

        assert2::let_assert!(Some(Err(error)) = result);
        assert!(error.to_string().contains(expected_error));
    }

    #[test]
    fn opt_build_when_io_error_returns_some_expected_err() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "test error");

        let result = Opt::build(Err(error));

        assert2::let_assert!(Some(Err(e)) = result);
        assert!(e.to_string().contains("test error"));
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

    fn dummy_dir_entry(file_name: &str) -> (TempDir, DirEntry) {
        let tmp_dir = TempDir::new().unwrap();
        let file_path = tmp_dir.path().join(file_name);
        std::fs::write(&file_path, "content").unwrap();
        let mut read_dir = std::fs::read_dir(tmp_dir.path()).unwrap();
        (tmp_dir, read_dir.next().unwrap().unwrap())
    }
}
