use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

#[cfg(not(test))]
pub trait Symlink {
    fn exec(&self) -> color_eyre::Result<()>;
    fn targets(&self) -> Vec<&Path>;
}

// std::any::Any bound is required only for test purposes
#[cfg(test)]
pub trait Symlink: std::any::Any + std::fmt::Debug {
    fn exec(&self) -> color_eyre::Result<()>;
    fn targets(&self) -> Vec<&Path>;
    fn as_any(&self) -> &dyn std::any::Any;
}

pub fn build<'a>(target: &'a str, link: Option<&'a str>) -> color_eyre::Result<Box<dyn Symlink>> {
    let Some(link) = link else {
        let target = PathBuf::from(target);
        if !target.is_file() {
            bail!("target {target:?} must be an existing file for SymlinkNoOp")
        }
        return Ok(Box::new(SymlinkNoOp { target }));
    };

    let target = PathBuf::from(target);
    if target.ends_with("*") {
        let link = PathBuf::from(link);
        if !link.is_dir() {
            bail!("link {link:?} must be an existing directory for SymlinkFilesIntoDir")
        }
        let mut targets = vec![];
        let parent = target
            .parent()
            .ok_or(eyre!("target {target:?} without parent"))?;
        for entry in std::fs::read_dir(parent)? {
            targets.push(entry?.path());
        }
        return Ok(Box::new(SymlinkFilesIntoDir {
            targets,
            link_dir: link,
        }));
    }

    if !target.is_file() {
        bail!(
            "target {target:?} must be an existing file for either SymlinkFileIntoDir or SymlinkFile"
        );
    }

    let link = PathBuf::from(link);
    if link.is_dir() {
        return Ok(Box::new(SymlinkFileIntoDir {
            target,
            link_dir: link,
        }));
    }

    Ok(Box::new(SymlinkFile { target, link }))
}

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct SymlinkNoOp {
    target: PathBuf,
}

impl Symlink for SymlinkNoOp {
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

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct SymlinkFile {
    target: PathBuf,
    link: PathBuf,
}

impl Symlink for SymlinkFile {
    fn exec(&self) -> color_eyre::Result<()> {
        // Remove existing link/file if exists
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

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct SymlinkFileIntoDir {
    target: PathBuf,
    link_dir: PathBuf,
}

impl Symlink for SymlinkFileIntoDir {
    fn exec(&self) -> color_eyre::Result<()> {
        let target_name = self
            .target
            .file_name()
            .ok_or_else(|| eyre!("target {:?} has no filename", self.target))?;
        let link_path = Path::new(&self.link_dir).join(target_name);
        if link_path.exists() {
            std::fs::remove_file(&link_path)?;
        }
        std::os::unix::fs::symlink(&self.target, &link_path)?;
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

#[cfg_attr(test, derive(Debug))]
pub struct SymlinkFilesIntoDir {
    targets: Vec<PathBuf>,
    link_dir: PathBuf,
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

impl Symlink for SymlinkFilesIntoDir {
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

#[cfg(test)]
mod tests {
    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn test_build_fails_if_target_is_not_an_existing_file_and_link_is_none() {
        let res = build("not_existing_file", None);

        assert2::let_assert!(Err(error) = res);

        pretty_assertions::assert_eq!(
            r#"target "not_existing_file" must be an existing file for SymlinkNoOp"#,
            error.to_string()
        );
    }

    #[test]
    fn test_build_builds_the_expected_symlink_no_op() {
        let target = NamedTempFile::new().unwrap();
        let target_path = target.into_temp_path();

        let res = build(target_path.to_str().unwrap(), None);

        assert2::let_assert!(Ok(symlink) = res);
        pretty_assertions::assert_eq!(
            Some(&SymlinkNoOp {
                target: target_path.to_path_buf()
            }),
            symlink.as_any().downcast_ref::<SymlinkNoOp>()
        );
    }

    #[test]
    fn test_build_builds_the_expected_symlink_files_into_dir() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let target = tmp_dir.path().join("*");
        let actual_targets = {
            let one = NamedTempFile::new_in(&tmp_dir).unwrap();
            let two = NamedTempFile::new_in(&tmp_dir).unwrap();
            vec![one, two]
        };
        let tmp_dir = tempfile::tempdir().unwrap();
        let link = tmp_dir.path().to_string_lossy();

        let res = build(target.to_str().unwrap(), Some(&link));

        assert2::let_assert!(Ok(symlink) = res);
        pretty_assertions::assert_eq!(
            Some(&SymlinkFilesIntoDir {
                targets: actual_targets.iter().map(|f| f.path().to_owned()).collect(),
                link_dir: link.into_owned().into()
            }),
            symlink.as_any().downcast_ref::<SymlinkFilesIntoDir>()
        );
    }

    #[test]
    fn test_build_fails_if_target_is_not_an_existing_file_and_link_is_suppiled() {
        let target = "inexistent_file";

        let res = build(target, Some("whatever"));

        assert2::let_assert!(Err(error) = res);
        pretty_assertions::assert_eq!(
            format!(
                "target {target:?} must be an existing file for either SymlinkFileIntoDir or SymlinkFile"
            ),
            error.to_string()
        );
    }

    #[test]
    fn test_build_builds_the_expected_symlink_file_into_dir() {
        let target = NamedTempFile::new().unwrap();
        let target_path = target.into_temp_path();
        let link_dir = tempfile::tempdir().unwrap();
        let link_dir_path = link_dir.path();

        let res = build(
            target_path.to_str().unwrap(),
            Some(link_dir_path.to_str().unwrap()),
        );

        assert2::let_assert!(Ok(symlink) = res);
        pretty_assertions::assert_eq!(
            Some(&SymlinkFileIntoDir {
                target: target_path.to_path_buf(),
                link_dir: link_dir_path.into()
            }),
            symlink.as_any().downcast_ref::<SymlinkFileIntoDir>()
        );
    }

    #[test]
    fn test_build_builds_the_expected_symlink_file() {
        let target = NamedTempFile::new().unwrap();
        let target_path = target.into_temp_path();
        let link_dir = tempfile::tempdir().unwrap();
        let link = link_dir.path().join("i_am_the_link");

        let res = build(target_path.to_str().unwrap(), Some(link.to_str().unwrap()));

        assert2::let_assert!(Ok(symlink) = res);
        pretty_assertions::assert_eq!(
            Some(&SymlinkFile {
                target: target_path.to_path_buf(),
                link
            }),
            symlink.as_any().downcast_ref::<SymlinkFile>()
        );
    }
}
