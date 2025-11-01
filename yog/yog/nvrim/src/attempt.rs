//! Exposes a dictionary with a `create_scratch_file` function for selecting and copying scratch files from the attempts
//! directory.

use std::fs::DirEntry;
use std::fs::ReadDir;
use std::path::Path;
use std::path::PathBuf;

use chrono::DateTime;
use chrono::Local;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;

const SCRATCHES_PATH_PARTS: &[&str] = &["yog", "nvrim", "src", "attempt"];

/// [`Dictionary`] of scratch file utilities.
pub fn dict() -> Dictionary {
    dict! {
        "create_scratch_file": fn_from!(create_scratch_file),
    }
}

/// Creates a scratch file by selecting and copying a template file.
///
/// This function retrieves available scratch files, presents a selection UI to the user,
/// and creates a new scratch file based on the selection inside a tmp folder.
fn create_scratch_file(_: ()) {
    let Ok(scratches_dir_content) = get_scratches_dir_content() else {
        return;
    };

    let scratches = scratches_dir_content
        .into_iter()
        .filter_map(|entry| {
            Scratch::from(entry)?
                .inspect_err(|error| ytil_nvim_oxi::api::notify_error(error))
                .ok()
        })
        .collect::<Vec<_>>();

    let dest_dir = Path::new("/tmp").join("attempt.rs");

    if let Err(error) = std::fs::create_dir_all(&dest_dir) {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot create dest dir | dest_dir={:?} error={error:#?}",
            dest_dir.display().to_string()
        ));
        return;
    }

    let callback = {
        let scratches = scratches.clone();
        move |choice_idx| {
            let Some(scratch): Option<&Scratch> = scratches.get(choice_idx) else {
                return;
            };
            let dest = scratch.dest_file_path(&dest_dir, Local::now());
            if let Err(error) = std::fs::copy(&scratch.path, &dest) {
                ytil_nvim_oxi::api::notify_error(format!(
                    "cannot copy file | from={} to={} error={error:#?}",
                    scratch.path.display(),
                    dest.display()
                ));
                return;
            }
            let _ = ytil_nvim_oxi::api::exec_vim_cmd("edit", Some(&[dest.display().to_string()]));
        }
    };

    if let Err(error) = ytil_nvim_oxi::api::vim_ui_select(
        scratches.iter().map(|scratch| scratch.display_name.as_str()),
        &[("prompt", "Create scratch file ")],
        callback,
    ) {
        ytil_nvim_oxi::api::notify_error(error);
    }
}

/// Retrieves the entries of the scratches directory.
///
/// # Returns
/// A generic result containing a [`ReadDir`] in case of success.
///
/// # Errors
/// Returns an error if the workspace root cannot be determined or the directory cannot be read.
fn get_scratches_dir_content() -> color_eyre::Result<ReadDir> {
    ytil_system::get_workspace_root()
        .map(|workspace_root| ytil_system::build_path(workspace_root, SCRATCHES_PATH_PARTS))
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(format!("cannot get workspace root | error={error:#?}"));
        })
        .and_then(|dir| std::fs::read_dir(dir).map_err(From::from))
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(format!("cannot read attempt files dir | error={error:#?}"));
        })
}

/// An available scratch file.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
struct Scratch {
    /// The name shown when selecting the scratch file.
    display_name: String,
    /// The base name of the scratch file without extension.
    base_name: String,
    /// The file extension of the scratch file.
    extension: String,
    /// The full path to the scratch file.
    path: PathBuf,
}

impl Scratch {
    /// Attempts to build a [`Scratch`] file from a [`DirEntry`] result.
    ///
    /// # Arguments
    /// - `read_dir_res` The result of reading a directory entry.
    ///
    /// # Returns
    /// - `Some(Ok(scratch))` if the entry is a valid file with all required components (name, stem, extension).
    /// - `Some(Err(error))` if an error occurs while extracting file components.
    /// - `None` if the entry is not a file.
    pub fn from(read_dir_res: std::io::Result<DirEntry>) -> Option<color_eyre::Result<Self>> {
        let path = match read_dir_res.map(|entry| entry.path()) {
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
            path,
        }))
    }

    /// Generates the destination file path for the scratch.
    ///
    /// The path is constructed as `{dest_dir}/{base_name}-{timestamp}.{extension}` where timestamp is a provided
    /// [`Local`] [`DateTime`].
    ///
    /// # Arguments
    /// - `dest_dir` The directory where the file should be placed.
    /// - `date_time` The date and time to use for the timestamp.
    ///
    /// # Returns
    /// The full path to the destination file.
    pub fn dest_file_path(&self, dest_dir: &Path, date_time: DateTime<Local>) -> PathBuf {
        dest_dir.join(format!(
            "{}-{}.{}",
            self.base_name,
            date_time.format("%Y%m%d-%H%M%S"),
            self.extension
        ))
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    #[case("test.txt", "test.txt", "test", "txt")]
    #[case(".hidden.txt", ".hidden.txt", ".hidden", "txt")]
    fn scratch_from_when_valid_file_returns_some_ok(
        #[case] file_name: &str,
        #[case] expected_display: &str,
        #[case] expected_base: &str,
        #[case] expected_ext: &str,
    ) {
        let (_tmp_dir, entry) = dummy_dir_entry(file_name);
        let expected_path = entry.path();

        let result = Scratch::from(Ok(entry));

        assert2::let_assert!(Some(Ok(actual)) = result);
        pretty_assertions::assert_eq!(
            actual,
            Scratch {
                display_name: expected_display.to_string(),
                base_name: expected_base.to_string(),
                extension: expected_ext.to_string(),
                path: expected_path,
            },
        );
    }

    #[test]
    fn scratch_from_when_directory_returns_none() {
        let temp_dir = TempDir::new().unwrap();
        let sub_dir = temp_dir.path().join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        let mut read_dir = std::fs::read_dir(temp_dir.path()).unwrap();
        let entry = read_dir.next().unwrap().unwrap();

        let result = Scratch::from(Ok(entry));

        assert!(result.is_none());
    }

    #[rstest]
    #[case("test", "missing extension")]
    #[case(".hidden", "missing extension")]
    fn scratch_from_when_invalid_file_returns_some_expected_error(
        #[case] file_name: &str,
        #[case] expected_error: &str,
    ) {
        let (_tmp_dir, entry) = dummy_dir_entry(file_name);

        let result = Scratch::from(Ok(entry));

        assert2::let_assert!(Some(Err(error)) = result);
        assert!(error.to_string().contains(expected_error));
    }

    #[test]
    fn scratch_from_when_io_error_returns_some_expected_err() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "test error");

        let result = Scratch::from(Err(error));

        assert2::let_assert!(Some(Err(e)) = result);
        assert!(e.to_string().contains("test error"));
    }

    #[test]
    fn scratch_dest_file_path_returns_expected_path() {
        let scratch = Scratch {
            display_name: "test.txt".to_string(),
            base_name: "test".to_string(),
            extension: "txt".to_string(),
            path: PathBuf::from("/some/path/test.txt"),
        };

        let date_time = Local.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let result = scratch.dest_file_path(Path::new("/tmp"), date_time);

        pretty_assertions::assert_eq!(result, PathBuf::from("/tmp/test-20230101-120000.txt"));
    }

    fn dummy_dir_entry(file_name: &str) -> (TempDir, DirEntry) {
        let tmp_dir = TempDir::new().unwrap();
        let file_path = tmp_dir.path().join(file_name);
        std::fs::write(&file_path, "content").unwrap();
        let mut read_dir = std::fs::read_dir(tmp_dir.path()).unwrap();
        (tmp_dir, read_dir.next().unwrap().unwrap())
    }
}
