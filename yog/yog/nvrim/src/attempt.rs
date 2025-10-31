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

    let target_dir = Path::new("/tmp").join("attempt.rs").to_path_buf();

    if let Err(error) = std::fs::create_dir_all(&target_dir) {
        ytil_nvim_oxi::api::notify_error(&format!(
            "cannot create target dir | target={:?} error={error:#?}",
            target_dir.display().to_string()
        ));
        return;
    };

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
                        "cannot copy file | from={:?} to={to:?} error={error:#?}",
                        opt.file_path
                    ));
                    return;
                };
                if let Err(error) = nvim_oxi::api::command(&format!("edit {}", to.display())) {
                    ytil_nvim_oxi::api::notify_error(&format!(
                        "cannot open file in new buffer | path={} error={error:#?}",
                        to.display()
                    ));
                };
            }
        },
    ) {
        ytil_nvim_oxi::api::notify_error(&format!("{error}"));
    }
}

#[derive(Clone, Debug)]
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
