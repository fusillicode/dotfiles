use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

#[cfg(not(test))]
pub trait LnSfOp {
    fn exec(&self) -> color_eyre::Result<()>;
    fn targets(&self) -> Vec<&Path>;
}

// std::any::Any bound is required only for test purposes
#[cfg(test)]
pub trait LnSfOp: std::any::Any + std::fmt::Debug {
    fn exec(&self) -> color_eyre::Result<()>;
    fn targets(&self) -> Vec<&Path>;
    fn as_any(&self) -> &dyn std::any::Any;
}

pub fn build_ls_sf_op<'a>(
    target: &'a str,
    link: Option<&'a str>,
) -> color_eyre::Result<Box<dyn LnSfOp>> {
    let Some(link) = link else {
        let target = PathBuf::from(target);
        if !target.is_file() {
            bail!("target {target:?} must be an existing file")
        }
        return Ok(Box::new(LnSfNoOp { target }));
    };

    let target = PathBuf::from(target);
    if target.ends_with("*") {
        let link = PathBuf::from(link);
        if !link.is_dir() {
            bail!("link {link:?} expected to point to an existing directory for LnSfFilesIntoDir")
        }
        let mut targets = vec![];
        let parent = target
            .parent()
            .ok_or(eyre!("target {target:?} without parent"))?;
        for entry in std::fs::read_dir(parent)? {
            targets.push(entry?.path());
        }
        return Ok(Box::new(LnSfFilesIntoDir {
            targets,
            link_dir: link,
        }));
    }

    if !target.is_file() {
        bail!("target {target:?} expected to point to an existing file");
    }

    let link = PathBuf::from(link);
    if link.is_dir() {
        return Ok(Box::new(LnSfFileIntoDir {
            target,
            link_dir: link,
        }));
    }

    Ok(Box::new(LnSfFile { target, link }))
}

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct LnSfNoOp {
    target: PathBuf,
}

impl LnSfOp for LnSfNoOp {
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
pub struct LnSfFile {
    target: PathBuf,
    link: PathBuf,
}

impl LnSfOp for LnSfFile {
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
pub struct LnSfFileIntoDir {
    target: PathBuf,
    link_dir: PathBuf,
}

impl LnSfOp for LnSfFileIntoDir {
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
pub struct LnSfFilesIntoDir {
    targets: Vec<PathBuf>,
    link_dir: PathBuf,
}

// Just for testing purposes
impl PartialEq for LnSfFilesIntoDir {
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

impl LnSfOp for LnSfFilesIntoDir {
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
    fn test_build_ls_sf_op_fails_if_target_is_not_an_existing_file_and_link_is_none() {
        let res = build_ls_sf_op("not_existing_file", None);

        assert2::let_assert!(Err(error) = res);

        pretty_assertions::assert_eq!(
            r#"target "not_existing_file" must be an existing file"#,
            error.to_string()
        );
    }

    #[test]
    fn test_build_ls_sf_op_builds_the_expected_ls_sf_no_op() {
        let target = NamedTempFile::new().unwrap();
        let target_path = target.into_temp_path();

        let res = build_ls_sf_op(target_path.to_str().unwrap(), None);

        assert2::let_assert!(Ok(ls_sf_op) = res);
        pretty_assertions::assert_eq!(
            Some(&LnSfNoOp {
                target: target_path.to_path_buf()
            }),
            ls_sf_op.as_any().downcast_ref::<LnSfNoOp>()
        );
    }

    #[test]
    fn test_build_ls_sf_op_builds_the_expected_ls_sf_files_into_dir() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let target = tmp_dir.path().join("*");
        let actual_targets = {
            let one = NamedTempFile::new_in(&tmp_dir).unwrap();
            let two = NamedTempFile::new_in(&tmp_dir).unwrap();
            vec![one, two]
        };
        let tmp_dir = tempfile::tempdir().unwrap();
        let link = tmp_dir.path().to_string_lossy();

        let res = build_ls_sf_op(target.to_str().unwrap(), Some(&link));

        assert2::let_assert!(Ok(ls_sf_op) = res);
        pretty_assertions::assert_eq!(
            Some(&LnSfFilesIntoDir {
                targets: actual_targets.iter().map(|f| f.path().to_owned()).collect(),
                link_dir: link.into_owned().into()
            }),
            ls_sf_op.as_any().downcast_ref::<LnSfFilesIntoDir>()
        );
    }

    #[test]
    fn test_build_ls_sf_op_fails_if_target_is_not_an_existing_file_and_link_is_suppiled() {
        let target = "inexistent_file";

        let res = build_ls_sf_op(target, Some("whatever"));

        assert2::let_assert!(Err(error) = res);
        pretty_assertions::assert_eq!(
            format!("target {target:?} expected to point to an existing file"),
            error.to_string()
        );
    }

    #[test]
    fn test_build_ls_sf_op_builds_the_expected_ls_sf_file_into_dir() {
        let target = NamedTempFile::new().unwrap();
        let target_path = target.into_temp_path();
        let link_dir = tempfile::tempdir().unwrap();
        let link_dir_path = link_dir.path();

        let res = build_ls_sf_op(
            target_path.to_str().unwrap(),
            Some(link_dir_path.to_str().unwrap()),
        );

        assert2::let_assert!(Ok(ls_sf_op) = res);
        pretty_assertions::assert_eq!(
            Some(&LnSfFileIntoDir {
                target: target_path.to_path_buf(),
                link_dir: link_dir_path.into()
            }),
            ls_sf_op.as_any().downcast_ref::<LnSfFileIntoDir>()
        );
    }

    #[test]
    fn test_build_ls_sf_op_builds_the_expected_ls_sf_file() {
        let target = NamedTempFile::new().unwrap();
        let target_path = target.into_temp_path();
        let link_dir = tempfile::tempdir().unwrap();
        let link = link_dir.path().join("i_am_the_link");

        let res = build_ls_sf_op(target_path.to_str().unwrap(), Some(link.to_str().unwrap()));

        assert2::let_assert!(Ok(ls_sf_op) = res);
        pretty_assertions::assert_eq!(
            Some(&LnSfFile {
                target: target_path.to_path_buf(),
                link
            }),
            ls_sf_op.as_any().downcast_ref::<LnSfFile>()
        );
    }
}
