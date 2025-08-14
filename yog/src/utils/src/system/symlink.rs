use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

#[cfg(not(test))]
pub trait SymlinkOp: std::fmt::Debug {
    fn exec(&self) -> color_eyre::Result<()>;
    fn targets(&self) -> Vec<&Path>;
}

// std::any::Any bound is required only for test purposes
#[cfg(test)]
pub trait SymlinkOp: std::any::Any + std::fmt::Debug {
    fn exec(&self) -> color_eyre::Result<()>;
    fn targets(&self) -> Vec<&Path>;
    fn as_any(&self) -> &dyn std::any::Any;
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub struct SymlinkNoOp {
    target: PathBuf,
}

impl SymlinkNoOp {
    pub fn new(target: &str) -> color_eyre::Result<Self> {
        Ok(Self {
            target: new_path_buf_if_file_exists(target)?,
        })
    }
}

impl SymlinkOp for SymlinkNoOp {
    fn exec(&self) -> color_eyre::Result<()> {
        Ok(())
    }

    fn targets(&self) -> Vec<&Path> {
        vec![&self.target]
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub struct SymlinkFile {
    target: PathBuf,
    link: PathBuf,
}

impl SymlinkFile {
    pub fn new<'a>(target: &'a str, link: &'a str) -> color_eyre::Result<Self> {
        let target = new_path_buf_if_file_exists(target)?;

        let link = PathBuf::from(link);
        let Some(link_parent) = link.parent() else {
            bail!("link {link:?} is the root directory, it must be an existing file");
        };
        if link_parent.to_string_lossy().is_empty() {
            bail!("parent of link {link:?} must not be empty");
        }
        if !link_parent.is_dir() {
            bail!("parent {link_parent:?} of link {link:?} must be an existing directory");
        }
        if link.is_dir() {
            bail!("link {link:?} is a directory, it must be an existing file");
        }

        Ok(Self { target, link })
    }
}

impl SymlinkOp for SymlinkFile {
    fn exec(&self) -> color_eyre::Result<()> {
        if self.link.exists() {
            std::fs::remove_file(&self.link)?;
        }
        std::os::unix::fs::symlink(&self.target, &self.link)?;
        Ok(())
    }

    fn targets(&self) -> Vec<&Path> {
        vec![&self.target]
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
pub struct SymlinkFilesIntoDir {
    targets: Vec<PathBuf>,
    link_dir: PathBuf,
}

impl SymlinkFilesIntoDir {
    pub fn new(target: &str, link_dir: &str) -> color_eyre::Result<Self> {
        let target = PathBuf::from(target);
        if !target.ends_with("*") {
            bail!("target {target:?} must end with glob pattern *");
        }

        let link = PathBuf::from(link_dir);
        if !link.is_dir() {
            bail!("link {link:?} must be an existing directory");
        }

        let mut targets = vec![];
        let parent = target
            .parent()
            .ok_or(eyre!("target {target:?} without parent"))?;
        for entry in std::fs::read_dir(parent)? {
            targets.push(entry?.path());
        }

        Ok(SymlinkFilesIntoDir {
            targets,
            link_dir: link,
        })
    }
}

impl SymlinkOp for SymlinkFilesIntoDir {
    fn exec(&self) -> color_eyre::Result<()> {
        for target in self.targets.iter() {
            if target.is_file() {
                let target_name = target
                    .file_name()
                    .ok_or_else(|| eyre!("target {target:?} has no filename"))?;
                let link_path = Path::new(&self.link_dir).join(target_name);
                if link_path.exists() {
                    std::fs::remove_file(&link_path)?;
                }
                std::os::unix::fs::symlink(target, &link_path)?;
            }
        }
        Ok(())
    }

    fn targets(&self) -> Vec<&Path> {
        self.targets.iter().map(AsRef::as_ref).collect()
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Just for testing purposes
impl PartialEq for SymlinkFilesIntoDir {
    fn eq(&self, other: &Self) -> bool {
        // Optimized impl to avoid unneeded cloning and sorting
        if self.link_dir == other.link_dir {
            let mut self_targets = self.targets.clone();
            self_targets.sort_unstable();
            let mut other_targets = other.targets.clone();
            other_targets.sort_unstable();
            return self_targets == other_targets;
        }
        false
    }
}

fn new_path_buf_if_file_exists(path: &str) -> color_eyre::Result<PathBuf> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        bail!("{path:?} must be an existing file");
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn test_symlink_file_new_fails_if_target_is_an_inexisting_file() {
        let target = "not_existing_file";

        let res = SymlinkFile::new(target, "whatever");

        assert2::let_assert!(Err(error) = res);
        pretty_assertions::assert_eq!(
            r#""not_existing_file" must be an existing file"#,
            error.to_string()
        );
    }

    #[test]
    fn test_symlink_file_new_fails_if_link_is_a_path_to_an_existing_directory() {
        let target = {
            let x = NamedTempFile::new().unwrap();
            x.into_temp_path()
        };
        let link_dir = tempfile::tempdir().unwrap();
        let link = link_dir.path();

        let res = SymlinkFile::new(target.to_str().unwrap(), link.to_str().unwrap());

        assert2::let_assert!(Err(error) = res);
        pretty_assertions::assert_eq!(
            format!(r#"link {link:?} is a directory, it must be an existing file"#),
            error.to_string()
        );
    }

    #[test]
    fn test_symlink_file_new_fails_if_link_is_a_path_to_an_inexisting_directory() {
        let target = {
            let x = NamedTempFile::new().unwrap();
            x.into_temp_path()
        };
        let link = "/inexistent/directory";

        let res = SymlinkFile::new(target.to_str().unwrap(), link);

        assert2::let_assert!(Err(error) = res);
        pretty_assertions::assert_eq!(
            format!(
                r#"parent "/inexistent" of link "/inexistent/directory" must be an existing directory"#
            ),
            error.to_string()
        );
    }

    #[test]
    fn test_symlink_file_new_succeeds_if_target_is_an_existing_file_and_link_an_existing_file() {
        let target = {
            let x = NamedTempFile::new().unwrap();
            x.into_temp_path()
        };
        let link = {
            let x = NamedTempFile::new().unwrap();
            x.into_temp_path()
        };

        let res = SymlinkFile::new(target.to_str().unwrap(), link.to_str().unwrap());

        assert2::let_assert!(Ok(symlink) = res);
        pretty_assertions::assert_eq!(
            Some(&SymlinkFile {
                target: target.to_path_buf(),
                link: link.to_path_buf(),
            }),
            symlink.as_any().downcast_ref::<SymlinkFile>()
        );
    }

    #[test]
    fn test_symlink_file_new_succeeds_if_target_is_an_existing_file_and_link_an_inexisting_file_in_an_existing_directory()
     {
        let target = {
            let x = NamedTempFile::new().unwrap();
            x.into_temp_path()
        };
        let link_dir = tempfile::tempdir().unwrap();
        let link = link_dir.path().join("inexistent_file");

        let res = SymlinkFile::new(target.to_str().unwrap(), link.to_str().unwrap());

        assert2::let_assert!(Ok(symlink) = res);
        pretty_assertions::assert_eq!(
            Some(&SymlinkFile {
                target: target.to_path_buf(),
                link: link.to_path_buf(),
            }),
            symlink.as_any().downcast_ref::<SymlinkFile>()
        );
    }

    #[test]
    fn test_symlink_files_into_dir_new_fails_if_target_is_not_a_glob_pattern() {
        let target = {
            let x = NamedTempFile::new().unwrap();
            x.into_temp_path()
        };

        let res = SymlinkFilesIntoDir::new(target.to_str().unwrap(), "whatever");

        assert2::let_assert!(Err(error) = res);
        pretty_assertions::assert_eq!(
            format!(
                r#"target {:?} must end with glob pattern *"#,
                target.to_path_buf()
            ),
            error.to_string()
        );
    }

    #[test]
    fn test_symlink_files_into_dir_new_fails_if_link_is_not_an_existing_directory() {
        let target = {
            let x = NamedTempFile::new().unwrap();
            x.into_temp_path().join("*")
        };
        let link_dir = tempfile::tempdir().unwrap();
        let link = link_dir.path().join("inexistent_dir");

        let res = SymlinkFilesIntoDir::new(target.to_str().unwrap(), link.to_str().unwrap());

        assert2::let_assert!(Err(error) = res);
        pretty_assertions::assert_eq!(
            format!(
                r#"link {:?} must be an existing directory"#,
                link.to_path_buf()
            ),
            error.to_string()
        );
    }

    #[test]
    fn test_symlink_files_into_dir_new_creates_the_expected_symlink_files_into_dir() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let target = tmp_dir.path().join("*");
        let actual_targets = {
            let one = NamedTempFile::new_in(&tmp_dir).unwrap();
            let two = NamedTempFile::new_in(&tmp_dir).unwrap();
            vec![one, two]
        };
        let tmp_dir = tempfile::tempdir().unwrap();
        let link = tmp_dir.path().to_string_lossy();

        let res = SymlinkFilesIntoDir::new(target.to_str().unwrap(), &link);

        assert2::let_assert!(Ok(symlink) = res);
        pretty_assertions::assert_eq!(
            Some(&SymlinkFilesIntoDir {
                targets: actual_targets.iter().map(|f| f.path().to_owned()).collect(),
                link_dir: link.into_owned().into()
            }),
            symlink.as_any().downcast_ref::<SymlinkFilesIntoDir>()
        );
    }
}
