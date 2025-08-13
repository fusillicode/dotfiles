use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::thread::JoinHandle;

use color_eyre::eyre;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

pub fn get_args() -> Vec<String> {
    let mut args = std::env::args();
    args.next();
    args.collect::<Vec<String>>()
}

pub fn join<T>(join_handle: JoinHandle<color_eyre::Result<T>>) -> Result<T, eyre::Error> {
    join_handle.join().map_err(|e| eyre!("join error {e:#?}"))?
}

pub fn cp_to_system_clipboard(content: &mut &[u8]) -> color_eyre::Result<()> {
    let mut pbcopy_child = crate::cmd::silent_cmd("pbcopy")
        .stdin(Stdio::piped())
        .spawn()?;
    std::io::copy(
        content,
        pbcopy_child
            .stdin
            .as_mut()
            .ok_or_else(|| eyre!("cannot get child stdin as mut"))?,
    )?;
    if !pbcopy_child.wait()?.success() {
        bail!("error copy content to system clipboard, content {content:#?}");
    }
    Ok(())
}

// Yes, `dir` is a `&str` and it's not sanitized but...I'm the alpha & the omega here!
pub fn chmod_x(dir: &str) -> color_eyre::Result<()> {
    Ok(crate::cmd::silent_cmd("sh")
        .args(["-c", &format!("chmod +x {dir}")])
        .status()?
        .exit_ok()?)
}

pub trait LnSf: std::any::Any {
    fn exec(&self) -> color_eyre::Result<()>;
    fn targets(&self) -> Vec<&Path>;
    fn as_any(&self) -> &dyn std::any::Any;
}

pub fn build_ls_sf_behavior<'a>(
    target: &'a str,
    link: Option<&'a str>,
) -> color_eyre::Result<Box<dyn LnSf>> {
    let Some(link) = link else {
        let target = PathBuf::from(target);
        if !target.is_file() {
            bail!("target {target:?} is not an existing file")
        }
        return Ok(Box::new(LnSfNoOp { target }));
    };

    let target = PathBuf::from(target);
    if target.ends_with("/*") {
        let link = PathBuf::from(link);
        if !link.is_dir() {
            bail!("link {link:?} expected to point to an existing directory for LnSfFilesIntoDir")
        }
        let mut targets = vec![];
        for entry in std::fs::read_dir(target)? {
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

pub struct LnSfFile {
    target: PathBuf,
    link: PathBuf,
}

impl LnSf for LnSfFile {
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct LnSfFileIntoDir {
    target: PathBuf,
    link_dir: PathBuf,
}

impl LnSf for LnSfFileIntoDir {
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct LnSfFilesIntoDir {
    targets: Vec<PathBuf>,
    link_dir: PathBuf,
}

impl LnSf for LnSfFilesIntoDir {
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct LnSfNoOp {
    target: PathBuf,
}

impl LnSf for LnSfNoOp {
    fn exec(&self) -> color_eyre::Result<()> {
        Ok(())
    }

    fn targets(&self) -> Vec<&Path> {
        vec![&self.target]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn rm_dead_symlinks(dir: &str) -> color_eyre::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() && std::fs::metadata(&path).is_err() {
            println!("🗑️ Removing dead symlink: {path:#?}");
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

pub fn rm_f<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    std::fs::remove_file(path).or_else(|error| {
        if std::io::ErrorKind::NotFound == error.kind() {
            return Ok(());
        }
        Err(error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_ls_sf_behavior_works_as_expected() {
        let res = build_ls_sf_behavior("not_existing_file", None);
        assert2::let_assert!(Err(error) = res);
        pretty_assertions::assert_eq!(
            r#"target "not_existing_file" is not an existing file"#,
            error.to_string()
        );

        let res = build_ls_sf_behavior("not_existing_file", None);
        assert2::let_assert!(Ok(ls_sf_op) = res);
        pretty_assertions::assert_eq!(
            Some(&LnSfNoOp {
                target: PathBuf::from("whatever")
            }),
            ls_sf_op.as_any().downcast_ref::<LnSfNoOp>()
        );
    }
}
