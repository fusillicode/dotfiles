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
}

pub struct LnSfFile {
    target: PathBuf,
    link: PathBuf,
}

impl LnSfFile {
    pub fn new(target: &str, link: &str) -> color_eyre::Result<Self> {
        let target = PathBuf::from(target);
        if target.exists() {
            bail!("target {target:?} does not exists");
        }
        if !target.is_file() {
            bail!("target {target:?} is not a file");
        }
        if link.ends_with("/") {
            bail!("link {link} is a directory, expected a file");
        }
        let link = PathBuf::from(link);
        Ok(Self { target, link })
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
}

pub struct LnSfFileIntoDir {
    target: PathBuf,
    link_dir: PathBuf,
}

impl LnSfFileIntoDir {
    pub fn new(target: &str, link_dir: &str) -> color_eyre::Result<Self> {
        let target = PathBuf::from(target);
        if target.exists() {
            bail!("target {target:?} does not exists");
        }
        if !target.is_file() {
            bail!("target {target:?} is not a file");
        }
        if !link_dir.ends_with("/") {
            bail!("link_dir {link_dir} is not a directory");
        }
        let link_dir = PathBuf::from(link_dir);
        Ok(Self { target, link_dir })
    }
}

impl LnSf for LnSfFileIntoDir {
    fn exec(&self) -> color_eyre::Result<()> {
        let target_file_name = self
            .target
            .file_name()
            .ok_or_else(|| eyre!("target has no filename"))?;
        let link_path = Path::new(&self.link_dir).join(target_file_name);
        if link_path.exists() {
            std::fs::remove_file(&link_path)?;
        }
        std::os::unix::fs::symlink(&self.target, &link_path)?;
        Ok(())
    }
}

pub struct LnSfFilesIntoDir {
    target_dir: PathBuf,
    link_dir: PathBuf,
}

impl LnSfFilesIntoDir {
    pub fn new(target_dir: &str, link_dir: &str) -> color_eyre::Result<Self> {
        if !target_dir.ends_with("/*") {
            bail!("target_dir {target_dir} without glob pattern *");
        }
        let target_dir = PathBuf::from(target_dir);
        if !link_dir.ends_with("/") {
            bail!("link_dir {link_dir} is not a directory");
        }
        let link_dir = PathBuf::from(link_dir);
        Ok(Self {
            target_dir,
            link_dir,
        })
    }
}

impl LnSf for LnSfFilesIntoDir {
    fn exec(&self) -> color_eyre::Result<()> {
        for entry_result in std::fs::read_dir(&self.link_dir)? {
            let entry = entry_result?;
            let path = entry.path();
            if path.is_file() {
                let file_name = path
                    .file_name()
                    .ok_or_else(|| eyre!("File has no filename"))?;
                let link_path = Path::new(&self.target_dir).join(file_name);
                if link_path.exists() {
                    std::fs::remove_file(&link_path)?;
                }
                std::os::unix::fs::symlink(&path, &link_path)?;
            }
        }
        Ok(())
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
