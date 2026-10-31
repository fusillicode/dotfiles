//! Shared code for symlinks, generated files, agent default locations, and sync outcomes.
//!
//! Matching symlinks need no changes. Warn and continue for conflicting symlinks,
//! files, or dirs.
//!
//! Identical generated files need no changes. Overwrite different generated regular
//! files. Create missing dirs needed by the supplied agents.

use std::fs;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;

use rootcause::Report;
use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_agents::agent::Agent;

#[derive(Debug)]
pub(super) enum SyncOutcome {
    Linked {
        source: PathBuf,
        destination: PathBuf,
    },
    Generated {
        destination: PathBuf,
    },
    Unchanged {
        destination: PathBuf,
    },
    Conflict {
        destination: PathBuf,
        operation: &'static str,
    },
    SubagentSkipped {
        error: Report,
    },
}

pub(super) fn agent_config_dir(home: &Path, agent: Agent) -> PathBuf {
    home.join(agent.root_path().iter().collect::<PathBuf>())
}

/// Create or reuse a real dir; return an outcome when linking must stop here.
///
/// A link to `linked_src_dir` records an unchanged outcome rather than being followed.
pub(super) fn prepare_dir(
    destination: &Path,
    label: &'static str,
    linked_src_dir: Option<&Path>,
) -> rootcause::Result<Option<SyncOutcome>> {
    match symlink_metadata(destination)? {
        Some(metadata) if metadata.is_dir() => Ok(None),
        Some(_) => {
            let already_linked = linked_src_dir.is_some_and(|src_dir| symlink_matches_source(src_dir, destination));

            if already_linked {
                Ok(Some(SyncOutcome::Unchanged {
                    destination: destination.to_path_buf(),
                }))
            } else {
                Ok(Some(SyncOutcome::Conflict {
                    destination: destination.to_path_buf(),
                    operation: label,
                }))
            }
        }
        None => {
            fs::create_dir_all(destination)
                .attach_with(|| format!("cannot create {label} directory | path={}", destination.display()))?;

            Ok(None)
        }
    }
}

/// Treat missing entries as optional while retaining metadata for dangling symlinks.
pub(super) fn symlink_metadata(path: &Path) -> rootcause::Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(report!(error).attach(path.display().to_string()).into()),
    }
}

pub(super) fn write_generated_file(
    contents: &str,
    destination: &Path,
    label: &'static str,
) -> rootcause::Result<SyncOutcome> {
    match symlink_metadata(destination)? {
        Some(metadata) if metadata.is_file() => {
            let existing = fs::read(destination)
                .attach_with(|| format!("cannot read {label} | path={}", destination.display()))?;

            if existing == contents.as_bytes() {
                return Ok(SyncOutcome::Unchanged {
                    destination: destination.to_path_buf(),
                });
            }
        }
        Some(_) => {
            return Ok(SyncOutcome::Conflict {
                destination: destination.to_path_buf(),
                operation: label,
            });
        }
        None => {}
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .attach_with(|| format!("cannot create {label} directory | path={}", parent.display()))?;
    }

    fs::write(destination, contents)
        .attach_with(|| format!("cannot write {label} | path={}", destination.display()))?;

    Ok(SyncOutcome::Generated {
        destination: destination.to_path_buf(),
    })
}

pub(super) fn create_symlink(source: &Path, destination: &Path) -> rootcause::Result<SyncOutcome> {
    match symlink_metadata(destination)? {
        Some(metadata) if metadata.is_symlink() && symlink_matches_source(source, destination) => {
            return Ok(SyncOutcome::Unchanged {
                destination: destination.to_path_buf(),
            });
        }
        Some(_) => {
            return Ok(SyncOutcome::Conflict {
                destination: destination.to_path_buf(),
                operation: "link",
            });
        }
        None => {}
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .attach_with(|| format!("cannot create destination directory | path={}", parent.display()))?;
    }

    symlink(source, destination).attach_with(|| {
        format!(
            "cannot create symlink | source={} destination={}",
            source.display(),
            destination.display()
        )
    })?;

    Ok(SyncOutcome::Linked {
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
    })
}

pub(super) fn symlink_matches_source(source: &Path, destination: &Path) -> bool {
    let Ok(resolved_source) = source.canonicalize() else {
        return false;
    };

    destination
        .canonicalize()
        .is_ok_and(|resolved| resolved == resolved_source)
}
