use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use rootcause::prelude::ResultExt;

use crate::cargo_metadata::Metadata;

/// Usage summary for CI subcommands.
const CI_USAGE: &str = "Usage: evoke ci [all | lint | test | release-native | audit]";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CmdKind {
    All,
    Audit,
    Lint,
    ReleaseNative,
    Test,
}

impl FromStr for CmdKind {
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "all" => Ok(Self::All),
            "audit" => Ok(Self::Audit),
            "lint" => Ok(Self::Lint),
            "release-native" => Ok(Self::ReleaseNative),
            "test" => Ok(Self::Test),
            unknown => rootcause::bail!("unknown evoke ci command: {unknown}\n{CI_USAGE}"),
        }
    }
}

impl CmdKind {
    pub fn run(self, workspace_root: &Path) -> rootcause::Result<()> {
        match self {
            Self::All => {
                Self::Lint.run(workspace_root)?;
                Self::Test.run(workspace_root)?;
                Self::ReleaseNative.run(workspace_root)?;
                Self::Audit.run(workspace_root)
            }
            Self::Audit => run_audit(workspace_root),
            Self::Lint => run_in_workspace(
                workspace_root,
                "cargo",
                &["run", "--quiet", "--bin", "tec", "--", "--all"],
            ),
            Self::ReleaseNative => run_native_release_build(workspace_root, &["build"]),
            Self::Test => run_test(workspace_root),
        }
    }
}

pub fn cmd_from_args(args: &[String]) -> rootcause::Result<Option<CmdKind>> {
    let Some(first) = args.first() else {
        return Ok(None);
    };
    if first != "ci" {
        return Ok(None);
    }

    let mut rest = args.iter().skip(1);
    let command = rest.next().map_or("all", String::as_str);
    if let Some(extra) = rest.next() {
        rootcause::bail!("unexpected extra evoke ci arg: {extra}\n{CI_USAGE}");
    }

    Ok(Some(command.parse()?))
}

fn run_audit(workspace_root: &Path) -> rootcause::Result<()> {
    let metadata = Metadata::read(workspace_root)?;
    run_native_auditable_build(workspace_root, &metadata)?;
    audit_native_bins(workspace_root, &metadata)
}

fn audit_native_bins(workspace_root: &Path, metadata: &Metadata) -> rootcause::Result<()> {
    let mut command = ytil_cmd::silent_cmd("cargo");
    command.args(["audit", "bin"]).current_dir(workspace_root);
    for bin_path in metadata.native_audit_bin_paths() {
        command.arg(bin_path);
    }
    run_command(&mut command)
}

fn run_test(workspace_root: &Path) -> rootcause::Result<()> {
    run_in_workspace(workspace_root, "rustup", &["component", "add", "llvm-tools-preview"])?;

    let repo_root = git_root(workspace_root)?;
    let rustflags = format!("--remap-path-prefix={repo_root}/=");
    let mut command = ytil_cmd::silent_cmd("cargo");
    command
        .args([
            "llvm-cov",
            "nextest",
            "--profile",
            "ci",
            "--workspace",
            "--all-features",
            "--lcov",
            "--output-path",
            "lcov.info",
        ])
        .current_dir(workspace_root)
        .env("CARGO_TARGET_DIR", "target/coverage")
        .env("RUSTFLAGS", rustflags);
    run_command(&mut command)
}

fn run_native_release_build(workspace_root: &Path, cargo_args: &[&str]) -> rootcause::Result<()> {
    let mut command = ytil_cmd::silent_cmd("cargo");
    command
        .args(cargo_args)
        .args(["--release", "--workspace"])
        .current_dir(workspace_root);
    run_command(&mut command)
}

fn run_native_auditable_build(workspace_root: &Path, metadata: &Metadata) -> rootcause::Result<()> {
    let package_names = metadata.native_bin_package_names();
    if package_names.is_empty() {
        rootcause::bail!("no native binary packages found for auditable build");
    }

    let mut command = ytil_cmd::silent_cmd("cargo");
    command
        .args(["auditable", "build", "--release"])
        .current_dir(workspace_root);
    for package_name in package_names {
        command.args(["--package", package_name]);
    }
    run_command(&mut command)
}

fn git_root(workspace_root: &Path) -> rootcause::Result<String> {
    let mut command = Command::new("git");
    command
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(workspace_root);
    let output = command.output().context("failed to spawn git rev-parse")?;
    output.status.exit_ok().context("git rev-parse failed")?;
    Ok(std::str::from_utf8(&output.stdout)
        .context("failed to decode git rev-parse stdout")?
        .trim()
        .into())
}

fn run_in_workspace(workspace_root: &Path, program: &str, args: &[&str]) -> rootcause::Result<()> {
    let mut command = ytil_cmd::silent_cmd(program);
    command.args(args).current_dir(workspace_root);
    run_command(&mut command)
}

fn run_command(command: &mut Command) -> rootcause::Result<()> {
    let command_debug = format!("{command:?}");
    command
        .status()
        .context("failed to spawn command")
        .attach_with(|| format!("command={command_debug}"))?
        .exit_ok()
        .context("command failed")
        .attach_with(|| format!("command={command_debug}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::without_ci_arg(&["--debug"], None)]
    #[case::default_all(&["ci"], Some(CmdKind::All))]
    fn test_cmd_from_args_returns_expected_command(#[case] input: &[&str], #[case] expected: Option<CmdKind>) {
        assert2::assert!(let Ok(command) = cmd_from_args(&args(input)));
        pretty_assertions::assert_eq!(command, expected);
    }

    #[rstest]
    #[case::audit("audit", CmdKind::Audit)]
    #[case::lint("lint", CmdKind::Lint)]
    #[case::release_native("release-native", CmdKind::ReleaseNative)]
    #[case::test("test", CmdKind::Test)]
    fn test_cmd_from_args_accepts_known_subcommands(#[case] subcommand: &str, #[case] expected: CmdKind) {
        assert!(matches!(cmd_from_args(&args(&["ci", subcommand])), Ok(Some(command)) if command == expected));
    }

    #[test]
    fn test_cmd_from_args_rejects_unknown_subcommand() {
        assert2::assert!(let Err(_) = cmd_from_args(&args(&["ci", "wat"])));
    }

    #[test]
    fn test_cmd_from_args_rejects_extra_arg() {
        assert2::assert!(let Err(_) = cmd_from_args(&args(&["ci", "lint", "extra"])));
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }
}
