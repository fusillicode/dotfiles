//! Link skills from `<src_dir>/skills/` one by one to vendor default locations.
//!
//! Include shared dirs such as `_shared`; skip files stored beside the skill dirs.
//!
//! Link each skill to `<vendor-config-dir>/skills/<skill-name>`.
//! For Codex, this is `~/.codex/skills/<skill-name>`.
//!
//! Link into existing vendor skills dirs. Keep matching symlinks. Warn and continue
//! for conflicting symlinks, files, or dirs.
//!
//! A symlink from the vendor skills dir to the source skills dir is already linked
//! and needs no changes.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_agents::agent::Agent as Vendor;

use crate::cmds::sync_harness::common;
use crate::cmds::sync_harness::common::SyncOutcome;

pub(super) struct SrcSkills {
    src_dir: PathBuf,
    skill_dirs: Vec<PathBuf>,
}

impl SrcSkills {
    pub(super) fn read_from(src_dir: &Path) -> rootcause::Result<Self> {
        let skill_dirs = collect_skill_dirs(src_dir)?;

        Ok(Self {
            src_dir: src_dir.to_path_buf(),
            skill_dirs,
        })
    }

    pub(super) fn link_for_vendor(&self, vendor: Vendor, home: &Path) -> Vec<rootcause::Result<SyncOutcome>> {
        if self.skill_dirs.is_empty() {
            return Vec::new();
        }

        let destination_dir = common::vendor_config_dir(home, vendor).join("skills");
        match common::prepare_dir(&destination_dir, "skills", Some(&self.src_dir)) {
            Ok(None) => {}
            Ok(Some(outcome)) => return vec![Ok(outcome)],
            Err(error) => return vec![Err(error)],
        }

        let mut outcomes = Vec::new();
        for skill_dir in &self.skill_dirs {
            let outcome = link_skill(skill_dir, &destination_dir);
            let failed = outcome.is_err();
            outcomes.push(outcome);

            if failed {
                break;
            }
        }

        outcomes
    }
}

fn link_skill(skill_dir: &Path, destination_dir: &Path) -> rootcause::Result<SyncOutcome> {
    let skill_name = skill_dir
        .file_name()
        .ok_or_else(|| report!("skill directory has no name"))?;
    let destination = destination_dir.join(skill_name);

    common::create_symlink(skill_dir, &destination)
}

fn collect_skill_dirs(src_dir: &Path) -> rootcause::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(src_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(report!(error)
                .attach(format!("cannot read skills | path={}", src_dir.display()))
                .into());
        }
    };
    let mut skills = Vec::new();

    for entry in entries {
        let path = entry?.path();
        let metadata = fs::metadata(&path).attach_with(|| format!("cannot inspect skill | path={}", path.display()))?;

        if metadata.is_dir() {
            skills.push(path);
        }
    }

    skills.sort();
    Ok(skills)
}
