//! Sync `AGENTS.md`, skills, and subagents from the supplied `src_dir` to the supplied vendors.
//!
//! Link `AGENTS.md` and skills to vendor default locations, with skills linked one
//! by one. Generate vendor-specific subagent files from the generic subagent files.
//!
//! Missing source files or dirs are skipped. An error in a subagent file prints a
//! warning and does not stop the other subagent files.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use rootcause::Report;
use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_agents::agent::Agent as Vendor;

use crate::cmds::sync_harness::agents_md::AgentsMd;
use crate::cmds::sync_harness::skills::SrcSkills;

mod output;

mod agents_md;
mod common;
mod skills;
mod subagents;

#[derive(Debug, Eq, PartialEq)]
pub struct Opts {
    pub src_dir: PathBuf,
    pub vendors: Vec<Vendor>,
}

impl TryFrom<Vec<OsString>> for Opts {
    type Error = Report;

    fn try_from(args: Vec<OsString>) -> Result<Self, Self::Error> {
        let [src_dir, vendors] = args.as_slice() else {
            return Err(report!(
                "agg sync-harness requires a source dir and a comma-separated vendor list"
            ));
        };

        if src_dir.is_empty() {
            return Err(report!("agg sync-harness requires a non-empty source dir"));
        }

        let vendors = vendors
            .to_str()
            .ok_or_else(|| report!("agg sync-harness vendor list must be valid UTF-8"))?
            .split(',')
            .map(Vendor::from_name)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            src_dir: PathBuf::from(src_dir),
            vendors,
        })
    }
}

pub fn run(options: &Opts, home: &Path, stdout: &mut impl Write, stderr: &mut impl Write) -> rootcause::Result<()> {
    let src_dir = fs::canonicalize(&options.src_dir)
        .context("cannot resolve source dir")
        .attach_with(|| format!("src_dir={}", options.src_dir.display()))?;

    if !src_dir.is_dir() {
        return Err(
            report!("agg sync-harness source must be a directory").attach(format!("src_dir={}", src_dir.display()))
        );
    }

    let agents_md = AgentsMd::read_from(&src_dir)?;
    let src_skills = SrcSkills::read_from(&src_dir.join("skills"))?;

    for vendor in &options.vendors {
        if let Some(agents_md) = &agents_md {
            let outcome = agents_md.sync_for_vendor(*vendor, home)?;
            output::write(&[outcome], stdout, stderr)?;
        }

        for outcome in src_skills.link_for_vendor(*vendor, home) {
            output::write(&[outcome?], stdout, stderr)?;
        }
    }

    let outcomes = subagents::sync_subagents(&src_dir.join("subagents"), home, &options.vendors);
    output::write(&outcomes, stdout, stderr)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use serde_json::Value;
    use test_that::prelude::*;

    use super::*;

    const GENERIC_SUBAGENT: &str = r#"---
name: reviewer
description: 'Reviews: changes'
vendors:
  codex:
    model: gpt-6
    features:
      shell_tool: false
  claude:
    model: sonnet
    tools: [Read, Grep]
  cursor:
    readonly: true
  gemini:
    temperature: 0.2
    max_turns: 5
  opencode:
    permission:
      edit: deny
---

Review changes, including `quotes` and "escapes".
---
Keep this body delimiter.
"#;

    #[rstest::rstest]
    #[case::codex(Vendor::Codex, ".codex/AGENTS.md", ".codex/skills")]
    #[case::claude(Vendor::Claude, ".claude/CLAUDE.md", ".claude/skills")]
    #[case::gemini(Vendor::Gemini, ".gemini/GEMINI.md", ".gemini/skills")]
    #[case::opencode(Vendor::Opencode, ".config/opencode/AGENTS.md", ".config/opencode/skills")]
    fn test_run_when_vendor_is_selected_links_agents_md_and_skills_one_by_one(
        #[case] vendor: Vendor,
        #[case] agents_md: &str,
        #[case] skills: &str,
    ) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![vendor],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("sync configuration");

        assert_link(&home.join(agents_md), &src_dir.join("AGENTS.md"));
        assert!(
            !fs::symlink_metadata(home.join(skills))
                .expect("skills directory")
                .is_symlink()
        );
        assert_link(&home.join(skills).join("review"), &src_dir.join("skills/review"));
        assert_link(&home.join(skills).join("_shared"), &src_dir.join("skills/_shared"));
        assert!(!home.join(skills).join("README.md").exists());
        assert!(!home.join(".agents").exists());
        assert_eq!(warnings, Vec::<u8>::new());
    }

    #[test]
    fn test_run_when_multiple_vendors_are_selected_links_each_vendor() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Claude, Vendor::Gemini],
        };

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("sync configuration");

        assert_link(&home.join(".claude/CLAUDE.md"), &src_dir.join("AGENTS.md"));
        assert_link(&home.join(".gemini/GEMINI.md"), &src_dir.join("AGENTS.md"));
        assert_link(&home.join(".claude/skills/review"), &src_dir.join("skills/review"));
        assert_link(&home.join(".gemini/skills/review"), &src_dir.join("skills/review"));
        assert!(!home.join(".codex").exists());
    }

    #[test]
    fn test_run_when_cursor_is_selected_writes_mirrored_rule_and_links_skills() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Cursor],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("configure cursor");

        assert_link(&home.join(".cursor/skills/review"), &src_dir.join("skills/review"));
        let rule = fs::read_to_string(home.join(".cursor/rules/agents.mdc")).expect("generated rule");
        insta::assert_snapshot!("cursor_rule", normalize_output(&rule, dir.path()));
        assert_eq!(warnings, Vec::<u8>::new());
    }

    #[test]
    fn test_run_when_cursor_source_changes_refreshes_its_mirrored_rule() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Cursor],
        };
        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("initial cursor rule");
        let destination = home.join(".cursor/rules/agents.mdc");
        fs::write(src_dir.join("AGENTS.md"), "updated instructions").expect("update source");
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("refresh cursor rule");
        run(&options, &home, &mut Vec::new(), &mut warnings).expect("repeat without changes");

        let rule = fs::read_to_string(&destination).expect("updated rule");
        insta::assert_snapshot!("cursor_rule_refreshed", normalize_output(&rule, dir.path()));
        assert_eq!(warnings, Vec::<u8>::new());
    }

    #[rstest::rstest]
    #[case::dir("directory")]
    #[case::symlink("symlink")]
    #[case::dangling_symlink("dangling")]
    fn test_run_when_cursor_rule_destination_conflicts_preserves_it_and_links_skills(#[case] kind: &str) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let destination = home.join(".cursor/rules/agents.mdc");
        fs::create_dir_all(home.join(".cursor/rules")).expect("rules directory");
        let other = dir.path().join("other.mdc");
        fs::write(&other, "local rule").expect("other rule");
        match kind {
            "directory" => fs::create_dir(&destination).expect("conflicting directory"),
            "symlink" => symlink(&other, &destination).expect("conflicting symlink"),
            "dangling" => symlink("missing.mdc", &destination).expect("dangling symlink"),
            _ => panic!("unknown test scenario"),
        }
        let metadata = fs::symlink_metadata(&destination).expect("existing metadata");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Cursor, Vendor::Claude],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("continue past rule conflict");

        let actual = fs::symlink_metadata(&destination).expect("preserved destination");
        assert_eq!(actual.file_type(), metadata.file_type());
        insta::assert_snapshot!(
            "foreign_cursor_rule_preserved",
            fs::read_to_string(&other).expect("other rule")
        );
        assert_link(&home.join(".cursor/skills/review"), &src_dir.join("skills/review"));
        assert_link(&home.join(".claude/CLAUDE.md"), &src_dir.join("AGENTS.md"));
        let warnings = String::from_utf8(warnings).expect("warning text");
        insta::assert_snapshot!(
            format!("cursor_rule_conflict_{kind}"),
            normalize_output(&warnings, dir.path())
        );
    }

    #[test]
    fn test_run_when_cursor_rule_is_manually_written_overwrites_it() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let destination = home.join(".cursor/rules/agents.mdc");
        fs::create_dir_all(home.join(".cursor/rules")).expect("rules directory");
        fs::write(&destination, "manually maintained rule").expect("existing rule");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Cursor],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("overwrite manual rule");

        let actual = fs::read_to_string(&destination).expect("generated rule");
        insta::assert_snapshot!("cursor_rule_replaced", normalize_output(&actual, dir.path()));
        assert_eq!(warnings, Vec::<u8>::new());
    }

    #[test]
    fn test_run_when_cursor_rule_belongs_to_another_source_overwrites_it() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let mut options = Opts {
            src_dir,
            vendors: vec![Vendor::Cursor],
        };
        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("initial cursor rule");
        let destination = home.join(".cursor/rules/agents.mdc");
        fs::create_dir(dir.path().join("other")).expect("other source parent");
        options.src_dir = create_src_dir(&dir.path().join("other"));
        fs::write(options.src_dir.join("AGENTS.md"), "other source instructions").expect("other source instructions");
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("replace other source rule");

        let rule = fs::read_to_string(&destination).expect("updated rule");
        insta::assert_snapshot!("cursor_rule_from_other_source", normalize_output(&rule, dir.path()));
        let warnings = String::from_utf8(warnings).expect("warning text");
        insta::assert_snapshot!("cursor_other_source_warnings", normalize_output(&warnings, dir.path()));
    }

    #[test]
    fn test_run_when_links_match_does_nothing_on_rerun() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Claude],
        };
        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("initial sync");
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("repeat sync");

        assert_link(&home.join(".claude/CLAUDE.md"), &src_dir.join("AGENTS.md"));
        assert_link(&home.join(".claude/skills/review"), &src_dir.join("skills/review"));
        assert_eq!(warnings, Vec::<u8>::new());
    }

    #[test]
    fn test_run_when_source_is_relative_creates_absolute_symlink_targets() {
        let dir = tempfile::tempdir_in(".").expect("relative test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Claude],
        };

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("sync relative source");

        assert!(
            fs::read_link(home.join(".claude/CLAUDE.md"))
                .expect("instruction link")
                .is_absolute()
        );
        assert_link(&home.join(".claude/skills/review"), &src_dir.join("skills/review"));
    }

    #[rstest::rstest]
    #[case::instructions_only("skills", ".claude/CLAUDE.md")]
    #[case::skills_only("AGENTS.md", ".claude/skills/review")]
    fn test_run_when_agents_md_or_skills_is_missing_links_the_other(#[case] missing: &str, #[case] expected: &str) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let missing = src_dir.join(missing);
        if missing.is_dir() {
            fs::remove_dir_all(&missing).expect("remove source dir");
        } else {
            fs::remove_file(&missing).expect("remove source file");
        }
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude],
        };

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("sync AGENTS.md or skills");

        assert!(
            fs::symlink_metadata(home.join(expected))
                .expect("linked AGENTS.md or skill")
                .is_symlink()
        );
    }

    #[rstest::rstest]
    #[case::other_target("other")]
    #[case::dangling_target("missing")]
    fn test_run_when_symlink_conflicts_warns_and_continues(#[case] target: &str) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let target = dir.path().join(target);
        fs::create_dir_all(dir.path().join("other")).expect("other target");
        fs::create_dir_all(home.join(".claude/skills")).expect("destination directory");
        symlink(&target, home.join(".claude/CLAUDE.md")).expect("conflicting instructions");
        symlink(&target, home.join(".claude/skills/review")).expect("conflicting skill");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Claude, Vendor::Gemini],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("continue past conflicts");

        assert_eq!(
            fs::read_link(home.join(".claude/CLAUDE.md")).expect("original link"),
            target
        );
        assert_eq!(
            fs::read_link(home.join(".claude/skills/review")).expect("original link"),
            target
        );
        assert_link(&home.join(".claude/skills/_shared"), &src_dir.join("skills/_shared"));
        assert_link(&home.join(".gemini/GEMINI.md"), &src_dir.join("AGENTS.md"));
        let warnings = String::from_utf8(warnings).expect("warning text");
        insta::assert_snapshot!(
            format!(
                "symlink_conflict_{}",
                target.file_name().expect("target name").to_string_lossy()
            ),
            normalize_output(&warnings, dir.path())
        );
    }

    #[test]
    fn test_run_when_regular_file_conflicts_preserves_file_and_links_other_entries() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        fs::create_dir_all(home.join(".claude/skills/review")).expect("existing skill");
        fs::write(home.join(".claude/CLAUDE.md"), "local instructions").expect("existing instructions");
        fs::write(home.join(".claude/skills/review/local.txt"), "local skill").expect("existing skill content");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Claude],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("preserve conflicts");

        insta::assert_snapshot!(
            "local_agents_md_preserved",
            fs::read_to_string(home.join(".claude/CLAUDE.md")).expect("local instructions")
        );
        insta::assert_snapshot!(
            "local_skill_preserved",
            fs::read_to_string(home.join(".claude/skills/review/local.txt")).expect("local skill")
        );
        assert_link(&home.join(".claude/skills/_shared"), &src_dir.join("skills/_shared"));
        let warnings = String::from_utf8(warnings).expect("warning text");
        insta::assert_snapshot!("regular_file_conflicts", normalize_output(&warnings, dir.path()));
    }

    #[test]
    fn test_run_when_skills_root_is_conflicting_symlink_preserves_target_and_continues() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        let other = dir.path().join("other");
        fs::create_dir(&other).expect("other skill root");
        fs::create_dir_all(home.join(".claude")).expect("vendor root");
        symlink(&other, home.join(".claude/skills")).expect("conflicting skills root");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Claude, Vendor::Gemini],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("continue past skills root conflict");

        assert_eq!(
            fs::read_link(home.join(".claude/skills")).expect("original root link"),
            other
        );
        assert_eq!(fs::read_dir(&other).expect("other directory").count(), 0);
        assert_link(&home.join(".claude/CLAUDE.md"), &src_dir.join("AGENTS.md"));
        assert_link(&home.join(".gemini/skills/review"), &src_dir.join("skills/review"));
        let warnings = String::from_utf8(warnings).expect("warning text");
        insta::assert_snapshot!("skills_root_conflict", normalize_output(&warnings, dir.path()));
    }

    #[test]
    fn test_run_when_existing_links_are_relative_accepts_matching_targets() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        fs::create_dir_all(home.join(".claude")).expect("vendor root");
        symlink("../../source/AGENTS.md", home.join(".claude/CLAUDE.md")).expect("relative instructions link");
        symlink("../../source/skills", home.join(".claude/skills")).expect("matching skills root link");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Claude],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("matching relative links");

        assert_eq!(
            fs::read_link(home.join(".claude/CLAUDE.md")).expect("original relative link"),
            Path::new("../../source/AGENTS.md")
        );
        assert_link(&home.join(".claude/skills"), &src_dir.join("skills"));
        assert_eq!(warnings, Vec::<u8>::new());
    }

    #[test]
    fn test_run_when_src_dir_is_empty_creates_no_vendor_files() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = dir.path().join("source");
        fs::create_dir(&src_dir).expect("empty source");
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude],
        };

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("empty configuration");

        assert!(!home.exists());
    }

    #[rstest::rstest]
    #[case::missing_source("missing")]
    #[case::source_file("file")]
    #[case::agents_md_dir("agents-md-dir")]
    #[case::skills_file("skills-file")]
    fn test_run_when_source_is_invalid_reports_error_before_linking(#[case] scenario: &str) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_src_dir(dir.path());
        match scenario {
            "missing" => fs::remove_dir_all(&src_dir).expect("remove source"),
            "file" => {
                fs::remove_dir_all(&src_dir).expect("remove source");
                fs::write(&src_dir, "not a directory").expect("source file");
            }
            "agents-md-dir" => {
                fs::remove_file(src_dir.join("AGENTS.md")).expect("remove AGENTS.md");
                fs::create_dir(src_dir.join("AGENTS.md")).expect("invalid AGENTS.md");
            }
            "skills-file" => {
                fs::remove_dir_all(src_dir.join("skills")).expect("remove skills");
                fs::write(src_dir.join("skills"), "not a directory").expect("invalid skills");
            }
            _ => panic!("unknown test scenario"),
        }
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude],
        };

        let actual = run(&options, &home, &mut Vec::new(), &mut Vec::new());

        assert_that!(actual, err(anything()));
        assert!(!home.exists());
    }

    #[rstest::rstest]
    #[case::codex(Vendor::Codex, ".codex/agents/reviewer.toml")]
    #[case::claude(Vendor::Claude, ".claude/agents/reviewer.md")]
    #[case::cursor(Vendor::Cursor, ".cursor/agents/reviewer.md")]
    #[case::gemini(Vendor::Gemini, ".gemini/agents/reviewer.md")]
    #[case::opencode(Vendor::Opencode, ".config/opencode/agents/reviewer.md")]
    fn test_run_when_vendor_is_selected_generates_vendor_subagent_file(#[case] vendor: Vendor, #[case] relative: &str) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(dir.path(), GENERIC_SUBAGENT);
        fs::write(src_dir.join("AGENTS.md"), "Personal instructions\n").expect("instructions");
        fs::create_dir_all(src_dir.join("skills/review")).expect("skill directory");
        fs::write(src_dir.join("skills/review/SKILL.md"), "Review skill\n").expect("skill");
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![vendor],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("sync AGENTS.md, skills, and subagents");

        let destination = home.join(relative);
        assert!(fs::symlink_metadata(&destination).expect("subagent metadata").is_file());
        let contents = fs::read_to_string(destination).expect("generated subagent");

        if vendor == Vendor::Codex {
            let _: toml::Value = toml::from_str(&contents).expect("valid TOML");
        } else {
            let (yaml, _) = contents
                .strip_prefix("---\n")
                .expect("frontmatter start")
                .split_once("---\n")
                .expect("frontmatter end");
            let _: Value = serde_saphyr::from_str(yaml).expect("valid YAML");
        }

        insta::assert_snapshot!(format!("vendor_subagent_{}", vendor.name()), contents);

        let root = common::vendor_config_dir(&home, vendor);
        let skills = root.join("skills");
        assert!(
            fs::symlink_metadata(skills.join("review"))
                .expect("skill link")
                .is_symlink()
        );
        assert_eq!(warnings, Vec::<u8>::new());
    }

    #[test]
    fn test_run_when_multiple_vendors_are_selected_generates_only_selected_formats() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(
            dir.path(),
            "---\nname: reviewer\ndescription: Review changes\n---\nPrompt\n",
        );
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude, Vendor::Codex],
        };

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("sync selected vendors");

        assert!(home.join(".claude/agents/reviewer.md").is_file());
        assert!(home.join(".codex/agents/reviewer.toml").is_file());
        assert!(!home.join(".cursor").exists());
        assert!(!home.join(".gemini").exists());
        assert!(!home.join(".config/opencode").exists());
    }

    #[test]
    fn test_run_when_subagent_changes_refreshes_regular_file_and_preserves_unrelated_subagents() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(dir.path(), GENERIC_SUBAGENT);
        let home = dir.path().join("home");
        let options = Opts {
            src_dir: src_dir.clone(),
            vendors: vec![Vendor::Claude],
        };
        fs::create_dir_all(home.join(".claude/agents")).expect("subagent directory");
        fs::write(home.join(".claude/agents/reviewer.md"), "Manual content").expect("old output");
        fs::write(home.join(".claude/agents/unrelated.md"), "Other agent").expect("unrelated subagent");

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("replace regular file");

        let destination = home.join(".claude/agents/reviewer.md");
        insta::assert_snapshot!(
            "subagent_replaced",
            fs::read_to_string(&destination).expect("initial output")
        );
        let initial_metadata = fs::metadata(&destination).expect("initial metadata");

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("repeat sync");

        assert_eq!(
            fs::metadata(&destination)
                .expect("repeat metadata")
                .modified()
                .expect("mtime"),
            initial_metadata.modified().expect("initial mtime")
        );
        fs::write(
            src_dir.join("subagents/input.md"),
            GENERIC_SUBAGENT.replace("Reviews: changes", "Updated description"),
        )
        .expect("source edit");

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("refresh subagent");

        insta::assert_snapshot!(
            "subagent_refreshed",
            fs::read_to_string(destination).expect("updated output")
        );
        insta::assert_snapshot!(
            "unrelated_subagent_preserved",
            fs::read_to_string(home.join(".claude/agents/unrelated.md")).expect("unrelated output")
        );
    }

    #[rstest::rstest]
    #[case::file_symlink("file-symlink")]
    #[case::dangling_symlink("dangling-symlink")]
    #[case::dir("directory")]
    #[case::root_symlink("root-symlink")]
    #[case::root_file("root-file")]
    fn test_run_when_subagent_destination_conflicts_warns_and_continues(#[case] conflict: &str) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(dir.path(), GENERIC_SUBAGENT);
        let home = dir.path().join("home");
        let root = home.join(".claude/agents");
        let destination = root.join("reviewer.md");
        let foreign = dir.path().join("foreign");
        fs::create_dir_all(&root).expect("subagent directory");
        fs::create_dir(&foreign).expect("foreign directory");
        fs::write(foreign.join("reviewer.md"), "Original").expect("foreign subagent");
        match conflict {
            "file-symlink" => {
                symlink(foreign.join("reviewer.md"), &destination).expect("file symlink");
            }
            "dangling-symlink" => {
                symlink(foreign.join("missing.md"), &destination).expect("dangling symlink");
            }
            "directory" => fs::create_dir(&destination).expect("conflicting directory"),
            "root-symlink" => {
                fs::remove_dir(&root).expect("empty directory");
                symlink(&foreign, &root).expect("root symlink");
            }
            "root-file" => {
                fs::remove_dir(&root).expect("empty directory");
                fs::write(&root, "Root file").expect("root file");
            }
            _ => panic!("unknown fixture"),
        }
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude, Vendor::Gemini],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("continue past conflict");

        assert!(home.join(".gemini/agents/reviewer.md").is_file());
        let warnings = String::from_utf8(warnings).expect("warning output");
        insta::assert_snapshot!(
            format!("subagent_conflict_{conflict}"),
            normalize_output(&warnings, dir.path())
        );
        insta::assert_snapshot!(
            "foreign_subagent_preserved",
            fs::read_to_string(foreign.join("reviewer.md")).expect("foreign subagent")
        );
        match conflict {
            "file-symlink" | "dangling-symlink" => {
                assert!(fs::symlink_metadata(destination).expect("preserved entry").is_symlink());
            }
            "directory" => assert!(destination.is_dir()),
            "root-symlink" => assert!(fs::symlink_metadata(root).expect("preserved root").is_symlink()),
            "root-file" => insta::assert_snapshot!(
                "subagent_root_file_preserved",
                fs::read_to_string(root).expect("preserved root")
            ),
            _ => panic!("unknown fixture"),
        }
    }

    #[rstest::rstest]
    #[case::no_frontmatter("no_frontmatter", "name: reviewer\nPrompt")]
    #[case::unclosed("unclosed", "---\nname: reviewer\ndescription: Review\nPrompt")]
    #[case::missing_name("missing_name", "---\ndescription: Review\n---\nPrompt")]
    #[case::empty_description("empty_description", "---\nname: reviewer\ndescription: ''\n---\nPrompt")]
    #[case::empty_body("empty_body", "---\nname: reviewer\ndescription: Review\n---\n \n")]
    #[case::path_escape("path_escape", "---\nname: ../escape\ndescription: Review\n---\nPrompt")]
    #[case::unknown_top_level(
        "unknown_top_level",
        "---\nname: reviewer\ndescription: Review\nmodel: sonnet\n---\nPrompt"
    )]
    #[case::unknown_vendor(
        "unknown_vendor",
        "---\nname: reviewer\ndescription: Review\nvendors:\n  typo: {}\n---\nPrompt"
    )]
    #[case::reserved_name(
        "reserved_name",
        "---\nname: reviewer\ndescription: Review\nvendors:\n  claude:\n    name: other\n---\nPrompt"
    )]
    #[case::reserved_mode(
        "reserved_mode",
        "---\nname: reviewer\ndescription: Review\nvendors:\n  opencode:\n    mode: primary\n---\nPrompt"
    )]
    #[case::invalid_toml_value(
        "invalid_toml_value",
        "---\nname: reviewer\ndescription: Review\nvendors:\n  codex:\n    model: null\n---\nPrompt"
    )]
    fn test_run_when_subagent_is_invalid_warns_and_syncs_other_files(#[case] scenario: &str, #[case] contents: &str) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(dir.path(), contents);
        fs::write(src_dir.join("AGENTS.md"), "Instructions").expect("valid instructions");
        fs::create_dir_all(src_dir.join("skills/review")).expect("valid skill");
        write_valid_subagents(&src_dir);
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude, Vendor::Codex],
        };

        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("continue past invalid subagent");

        let warnings = String::from_utf8(warnings).expect("warning text");
        insta::assert_snapshot!(
            format!("invalid_subagent_{scenario}"),
            normalize_error(&warnings, dir.path())
        );
        assert_link(&home.join(".claude/CLAUDE.md"), &options.src_dir.join("AGENTS.md"));
        assert_link(
            &home.join(".claude/skills/review"),
            &options.src_dir.join("skills/review"),
        );
        assert_valid_subagents(&home);
        assert!(!home.join(".claude/agents/reviewer.md").exists());
        assert!(!home.join(".codex/agents/reviewer.toml").exists());
    }

    #[test]
    fn test_run_when_subagent_names_repeat_warns_and_preserves_first_definition() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(dir.path(), GENERIC_SUBAGENT);
        fs::write(
            src_dir.join("subagents/duplicate.md"),
            GENERIC_SUBAGENT.replace("Reviews: changes", "First definition"),
        )
        .expect("duplicate definition");
        write_valid_subagents(&src_dir);
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude, Vendor::Codex],
        };

        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("continue past duplicate");

        let warnings = String::from_utf8(warnings).expect("warning text");
        insta::assert_snapshot!("duplicate_subagent_name", normalize_error(&warnings, dir.path()));
        insta::assert_snapshot!(
            "first_duplicate_subagent",
            fs::read_to_string(home.join(".claude/agents/reviewer.md")).expect("first definition output")
        );
        assert_valid_subagents(&home);
    }

    #[test]
    fn test_run_when_subagents_dir_has_no_subagent_files_creates_no_vendor_files() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = dir.path().join("source");
        fs::create_dir_all(src_dir.join("subagents/nested")).expect("nested directory");
        fs::write(src_dir.join("subagents/README.txt"), "Not a subagent").expect("other file");
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude],
        };

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("empty subagent set");

        assert!(!home.exists());
    }

    #[test]
    fn test_run_when_frontmatter_uses_crlf_preserves_multiline_values_and_body() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(
            dir.path(),
            "---\r\nname: code_reader\r\ndescription: |-\r\n  Read code.\r\n  Report findings.\r\n---\r\n\r\nRead café paths: C:\\code\\\"quoted\".\r\n",
        );
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Codex, Vendor::Claude],
        };

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("sync CRLF definition");

        let codex = fs::read_to_string(home.join(".codex/agents/code_reader.toml")).expect("Codex output");
        let actual: toml::Value = toml::from_str(&codex).expect("Codex TOML");
        // Debug retains CRLF escapes, which text snapshots otherwise normalize.
        insta::assert_debug_snapshot!("codex_crlf_values", actual);
        insta::assert_snapshot!("codex_crlf_subagent", codex);
        let claude = fs::read_to_string(home.join(".claude/agents/code_reader.md")).expect("Claude output");
        insta::assert_debug_snapshot!("claude_crlf_subagent", claude);
    }

    #[test]
    fn test_run_when_multiple_subagent_files_exist_generates_each_subagent() {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(dir.path(), GENERIC_SUBAGENT);
        fs::write(
            src_dir.join("subagents/second.md"),
            "---\nname: implementer\ndescription: Implement changes\n---\nImplement.\n",
        )
        .expect("second subagent");
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Opencode],
        };

        run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("sync multiple definitions");

        assert!(home.join(".config/opencode/agents/reviewer.md").is_file());
        assert!(home.join(".config/opencode/agents/implementer.md").is_file());
    }

    #[rstest::rstest]
    #[case::read_error(vec![0xFF])]
    #[case::write_error(GENERIC_SUBAGENT.replace("name: reviewer", &format!("name: {}", "a".repeat(256))).into_bytes())]
    fn test_run_when_subagent_file_io_fails_warns_and_syncs_other_files(#[case] contents: Vec<u8>) {
        let dir = tempfile::tempdir().expect("test directory");
        let src_dir = create_subagent_src_dir(dir.path(), GENERIC_SUBAGENT);
        write_valid_subagents(&src_dir);
        fs::write(src_dir.join("subagents/input.md"), contents).expect("failing subagent file");
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Claude, Vendor::Codex],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("continue past file error");

        assert_ne!(warnings, Vec::<u8>::new());
        assert_valid_subagents(&home);
    }

    #[rstest::rstest]
    #[case::first_sync(false)]
    #[case::already_synced(true)]
    fn test_run_when_sync_succeeds_prints_each_outcome(#[case] already_synced: bool) {
        let dir = tempfile::tempdir().expect("test dir");
        let src_dir = create_src_dir(dir.path());
        fs::create_dir(src_dir.join("subagents")).expect("subagents dir");
        fs::write(src_dir.join("subagents/reviewer.md"), GENERIC_SUBAGENT).expect("subagent definition");
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Codex, Vendor::Claude, Vendor::Cursor],
        };
        if already_synced {
            run(&options, &home, &mut Vec::new(), &mut Vec::new()).expect("initial sync");
        }
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        run(&options, &home, &mut stdout, &mut stderr).expect("sync harness");

        let stdout = String::from_utf8(stdout).expect("success output");
        insta::assert_snapshot!(
            format!("successful_sync_already_synced_{already_synced}"),
            normalize_output(&stdout, dir.path())
        );
        assert_eq!(stderr, Vec::<u8>::new());
    }

    #[test]
    fn test_run_when_later_vendor_fails_preserves_completed_outcomes() {
        let dir = tempfile::tempdir().expect("test dir");
        let src_dir = create_src_dir(dir.path());
        let home = dir.path().join("home");
        fs::create_dir(&home).expect("home dir");
        fs::write(home.join(".claude"), "blocks vendor dir").expect("blocked vendor dir");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Codex, Vendor::Claude],
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let result = run(&options, &home, &mut stdout, &mut stderr);

        assert_that!(result, err(anything()));
        let stdout = String::from_utf8(stdout).expect("success output");
        insta::assert_snapshot!("completed_outcomes_before_error", normalize_output(&stdout, dir.path()));
        assert_eq!(stderr, Vec::<u8>::new());
    }

    #[test]
    fn test_run_when_codex_has_nested_settings_preserves_all_settings() {
        let dir = tempfile::tempdir().expect("test dir");
        let src_dir = create_subagent_src_dir(
            dir.path(),
            "---\nname: reader\ndescription: Read code\nvendors:\n  codex:\n    sandbox_mode: read-only\n    approval_policy: never\n    model_provider: custom\n    skills:\n      config:\n        - path: /skills/review/SKILL.md\n          enabled: false\n    mcp_servers:\n      logs:\n        url: https://example.com/mcp\n        enabled: true\n---\nRead code.\n",
        );
        let home = dir.path().join("home");
        let options = Opts {
            src_dir,
            vendors: vec![Vendor::Codex, Vendor::Claude],
        };
        let mut warnings = Vec::new();

        run(&options, &home, &mut Vec::new(), &mut warnings).expect("sync nested Codex settings");

        let codex = fs::read_to_string(home.join(".codex/agents/reader.toml")).expect("Codex subagent");
        let _: toml::Value = toml::from_str(&codex).expect("valid Codex TOML");
        insta::assert_snapshot!("codex_nested_subagent_settings", codex);
        let claude = fs::read_to_string(home.join(".claude/agents/reader.md")).expect("Claude subagent");
        insta::assert_snapshot!("claude_without_codex_subagent_settings", claude);
        assert_eq!(warnings, Vec::<u8>::new());
    }

    fn write_valid_subagents(src_dir: &Path) {
        for name in ["a_valid", "z_valid"] {
            fs::write(
                src_dir.join("subagents").join(format!("{name}.md")),
                format!("---\nname: {name}\ndescription: Valid agent\n---\nValid instructions\n"),
            )
            .expect("valid subagent file");
        }
    }

    fn assert_valid_subagents(home: &Path) {
        for name in ["a_valid", "z_valid"] {
            assert!(home.join(".claude/agents").join(format!("{name}.md")).is_file());
            assert!(home.join(".codex/agents").join(format!("{name}.toml")).is_file());
        }
    }

    fn normalize_error(error: &str, root: &Path) -> String {
        let error = normalize_output(error, root);

        error
            .lines()
            .filter(|line| !line.contains("crates/agg/src/cmds/sync_harness/subagents.rs:"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn create_subagent_src_dir(root: &Path, contents: &str) -> PathBuf {
        let src_dir = root.join("source");
        fs::create_dir_all(src_dir.join("subagents")).expect("source subagents");
        fs::write(src_dir.join("subagents/input.md"), contents).expect("source subagent");
        src_dir
    }

    fn normalize_output(output: &str, root: &Path) -> String {
        let root = root.canonicalize().expect("canonical test root");
        output.replace(&root.display().to_string(), "[ROOT]")
    }

    fn create_src_dir(dir: &Path) -> PathBuf {
        let src_dir = dir.join("source");
        fs::create_dir_all(src_dir.join("skills/review")).expect("source skill");
        fs::create_dir_all(src_dir.join("skills/_shared")).expect("shared skill resources");
        fs::write(src_dir.join("AGENTS.md"), "shared instructions").expect("source AGENTS.md");
        fs::write(src_dir.join("skills/review/SKILL.md"), "review changes").expect("source skill definition");
        fs::write(src_dir.join("skills/README.md"), "skill index").expect("source skill index");
        src_dir
    }

    fn assert_link(destination: &Path, src: &Path) {
        assert!(fs::symlink_metadata(destination).expect("link metadata").is_symlink());
        assert_eq!(
            destination.canonicalize().expect("link target"),
            src.canonicalize().expect("source target")
        );
    }
}
