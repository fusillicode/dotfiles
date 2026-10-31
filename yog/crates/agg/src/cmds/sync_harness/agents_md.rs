//! Link `AGENTS.md` from the supplied source dir to agent default locations under home:
//!
//! - `~/.codex/AGENTS.md`
//! - `~/.claude/CLAUDE.md`
//! - `~/.gemini/GEMINI.md`
//! - `~/.config/opencode/AGENTS.md`
//!
//! Cursor requires a generated rule at `~/.cursor/rules/agents.mdc` instead. Include
//! the `AGENTS.md` contents, its source dir, and `alwaysApply: true`.
//!
//! Overwrite a different regular rule file; preserve conflicting symlinks and dirs
//! as conflicts.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_agents::agent::Agent;

use crate::cmds::sync_harness::common;
use crate::cmds::sync_harness::common::SyncOutcome;

pub(super) struct AgentsMd {
    path: PathBuf,
}

impl AgentsMd {
    pub(super) fn read_from(src_dir: &Path) -> rootcause::Result<Option<Self>> {
        let path = src_dir.join("AGENTS.md");

        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(Self { path })),
            Ok(_) => Err(report!("source AGENTS.md must be a file").attach(path.display().to_string())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(report!(error).attach(path.display().to_string()).into()),
        }
    }

    pub(super) fn sync_for_agent(&self, agent: Agent, home: &Path) -> rootcause::Result<SyncOutcome> {
        match target_for_agent(agent, home) {
            AgentsMdTarget::Symlink(destination) => common::create_symlink(&self.path, &destination),
            AgentsMdTarget::CursorRule(destination) => write_cursor_rule(&self.path, &destination),
        }
    }
}

enum AgentsMdTarget {
    Symlink(PathBuf),
    CursorRule(PathBuf),
}

fn target_for_agent(agent: Agent, home: &Path) -> AgentsMdTarget {
    let config_dir = common::agent_config_dir(home, agent);

    match agent {
        Agent::Claude => AgentsMdTarget::Symlink(config_dir.join("CLAUDE.md")),
        Agent::Codex | Agent::Opencode => AgentsMdTarget::Symlink(config_dir.join("AGENTS.md")),
        Agent::Gemini => AgentsMdTarget::Symlink(config_dir.join("GEMINI.md")),
        Agent::Cursor => AgentsMdTarget::CursorRule(config_dir.join("rules/agents.mdc")),
    }
}

fn write_cursor_rule(agents_md_path: &Path, destination: &Path) -> rootcause::Result<SyncOutcome> {
    let agents_md_contents = fs::read_to_string(agents_md_path)
        .attach_with(|| format!("cannot read AGENTS.md | path={}", agents_md_path.display()))?;
    let description = format!(
        "Personal defaults mirrored from {}; source is authoritative.",
        agents_md_path.display()
    );
    let description = serde_json::to_string(&description)?;
    let header = format!("---\ndescription: {description}\nalwaysApply: true\n---\n\n");
    let src_dir = agents_md_path
        .parent()
        .ok_or_else(|| report!("AGENTS.md has no parent dir"))?;
    let contents = format!(
        "{header}Use Cursor's native skill and subagent invocation syntax for the operations named in the source instructions.\n\
         Resolve relative instruction paths from `{}`.\n\n{agents_md_contents}",
        src_dir.display()
    );

    common::write_generated_file(&contents, destination, "Cursor rule")
}
