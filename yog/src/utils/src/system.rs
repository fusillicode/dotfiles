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

pub trait LnSf {
    fn exec(&self) -> color_eyre::Result<()>;

    fn targets(&self) -> Vec<&Path>;

    fn into_path_buf_if_not_dir(field: &str, path: &str) -> color_eyre::Result<PathBuf> {
        (!path.ends_with("/"))
            .then_some(path)
            .ok_or_else(|| eyre!("{field} {path} is a directory, expected a file"))
            .map(PathBuf::from)
    }

    fn into_path_buf_if_dir(field: &str, path: &str) -> color_eyre::Result<PathBuf> {
        path.ends_with("/")
            .then_some(path)
            .ok_or_else(|| eyre!("{field} {path} is not a directory"))
            .map(PathBuf::from)
    }

    fn into_path_buf_if_file_and_exists(field: &str, path: &str) -> color_eyre::Result<PathBuf> {
        let path = PathBuf::from(path);
        if path.exists() {
            bail!("{field} {path:?} does not exists");
        }
        if !path.is_file() {
            bail!("{field} {path:?} is not a file");
        }
        Ok(path)
    }
}

pub struct LnSfFile {
    target: PathBuf,
    link: PathBuf,
}

impl LnSfFile {
    pub fn new(target: &str, link: &str) -> color_eyre::Result<Self> {
        Ok(Self {
            target: Self::into_path_buf_if_file_and_exists("target", target)?,
            link: Self::into_path_buf_if_not_dir("link", link)?,
        })
    }
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
}

pub struct LnSfFileIntoDir {
    target: PathBuf,
    link_dir: PathBuf,
}

impl LnSfFileIntoDir {
    pub fn new(target: &str, link_dir: &str) -> color_eyre::Result<Self> {
        Ok(Self {
            target: Self::into_path_buf_if_file_and_exists("target", target)?,
            link_dir: Self::into_path_buf_if_dir("link_dir", link_dir)?,
        })
    }
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
}

pub struct LnSfFilesIntoDir {
    targets: Vec<PathBuf>,
    link_dir: PathBuf,
}

impl LnSfFilesIntoDir {
    pub fn new(target_dir: &str, link_dir: &str) -> color_eyre::Result<Self> {
        let target_dir = target_dir
            .ends_with("/*")
            .then_some(target_dir)
            .ok_or_else(|| eyre!("target_dir {target_dir} is not a glob pattern *"))
            .map(PathBuf::from)?;
        let mut targets = vec![];
        for target in std::fs::read_dir(target_dir)? {
            targets.push(target?.path());
        }

        Ok(Self {
            targets,
            link_dir: Self::into_path_buf_if_dir("link_dir", link_dir)?,
        })
    }
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
}

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
    #[test]
    fn test_ln_sf_file_new_works_as_expected() {}

    #[test]
    fn test_ln_sf_file_into_dir_new_works_as_expected() {}

    #[test]
    fn test_ln_sf_files_into_dir_new_works_as_expected() {}
}
