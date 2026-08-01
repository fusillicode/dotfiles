//! Implementation of `frs repo fix`.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::thread;

use git2::Repository as GitRepo;
use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use ytil_sys::pico_args::Arguments;

mod quarantine;

const DEFAULT_JOBS: usize = 7;

/// Runs the `frs repo fix` command.
///
/// # Errors
/// - Repo maintenance fails.
pub fn run(mut cli_args: Arguments) -> rootcause::Result<()> {
    if cli_args.contains("--help") {
        print!("{}", include_str!("../../repo-fix-help.txt"));
        return Ok(());
    }
    fix(&RepoFixOpts::try_from(cli_args.finish())?)
}

#[derive(Debug)]
struct RepoFixOpts {
    directory: PathBuf,
    clean: bool,
    jobs: usize,
}

impl TryFrom<Vec<OsString>> for RepoFixOpts {
    type Error = rootcause::Report;

    fn try_from(raw: Vec<OsString>) -> Result<Self, Self::Error> {
        let mut before_dash_dash = Vec::new();
        let mut after_dash_dash = Vec::new();
        let mut after_separator = false;
        for argument in raw {
            if after_separator {
                after_dash_dash.push(argument);
            } else if argument == "--" {
                after_separator = true;
            } else {
                before_dash_dash.push(argument);
            }
        }

        let mut cli_args = Arguments::from_vec(before_dash_dash);
        let mut clean = false;
        while cli_args.contains("--clean") {
            clean = true;
        }
        let mut jobs = DEFAULT_JOBS;
        while let Some(value) = cli_args
            .opt_value_from_str::<_, usize>("--jobs")
            .map_err(|error| report!("--jobs requires a positive integer").attach(error.to_string()))?
        {
            if value == 0 {
                return Err(report!("--jobs must be a positive integer"));
            }
            jobs = value;
        }

        let mut positionals = cli_args.finish();
        if let Some(option) = positionals
            .iter()
            .find(|argument| argument.to_string_lossy().starts_with('-'))
        {
            return Err(report!("unknown repo fix option").attach(format!("option={}", option.to_string_lossy())));
        }
        positionals.append(&mut after_dash_dash);
        let [directory] = positionals.as_slice() else {
            return Err(report!("expected exactly one repo directory"));
        };
        Ok(Self {
            directory: PathBuf::from(directory),
            clean,
            jobs,
        })
    }
}

#[derive(Clone, Debug)]
struct Workspace {
    repo: PathBuf,
    root: PathBuf,
    target: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    workspace_root: PathBuf,
    target_directory: PathBuf,
}

#[derive(Debug)]
enum Failure {
    Repo { repo: PathBuf, message: String },
    Quarantine { repo: PathBuf, failure: rootcause::Report },
    Traversal { message: String },
}

impl Failure {
    fn repo(repo: PathBuf, message: impl Into<String>) -> Self {
        Self::Repo {
            repo,
            message: message.into(),
        }
    }
}

struct ManifestDiscovery {
    manifests: Vec<PathBuf>,
    failures: Vec<Failure>,
}

struct RepoDiscovery {
    manifest_count: usize,
    repos: Vec<PathBuf>,
    workspaces: Vec<Workspace>,
    skipped_manifests: Vec<PathBuf>,
    failures: Vec<Failure>,
}

/// Configures Cargo target directories and removes quarantine metadata below `directory`.
///
/// # Errors
/// - The directory or a required macOS/Cargo command is unavailable.
/// - Confirmation input cannot be read, discovery is incomplete, or any repo operation fails.
fn fix(opts: &RepoFixOpts) -> rootcause::Result<()> {
    let directory = validate_directory(&opts.directory)?;
    require_command("cargo")?;
    require_command("tmutil")?;
    require_command("xattr")?;

    println!(
        "{} below: {}",
        if opts.clean {
            "Preparing workspace build directories, quarantine cleanup, and cargo clean"
        } else {
            "Preparing workspace build directories and quarantine cleanup"
        }
        .blue()
        .bold(),
        directory.display()
    );
    print!("{} ", "Continue? [y/N]".yellow().bold());

    std::io::Write::flush(&mut std::io::stdout())?;
    let mut confirmation = String::new();
    std::io::stdin().read_line(&mut confirmation)?;
    if !matches!(confirmation.trim(), "y" | "Y") {
        println!("{}", "Cancelled before making changes".yellow().bold());
        return Ok(());
    }

    let manifest_discovery = collect_manifest_paths(&directory);
    let repo_discovery = discover_repos(&manifest_discovery.manifests);
    let mut failures = Vec::new();
    failures.extend(manifest_discovery.failures);
    failures.extend(repo_discovery.failures);

    for manifest in repo_discovery.skipped_manifests {
        eprintln!(
            "{} skipping Cargo manifest outside a Git repo: {}",
            "Warning".yellow().bold(),
            manifest.display()
        );
    }
    println!(
        "{} {} Cargo manifest(s) in {} workspace(s) across {} Git repo(s)",
        "Found".blue().bold(),
        repo_discovery.manifest_count,
        repo_discovery.workspaces.len(),
        repo_discovery.repos.len()
    );

    if opts.clean {
        for workspace in &repo_discovery.workspaces {
            println!("{} {}", "Running cargo clean".blue().bold(), workspace.root.display());
        }
        for cleanup in clean_workspaces(&repo_discovery.workspaces, opts.jobs) {
            match cleanup {
                Ok(workspace) => println!("{} {}", "Cargo clean complete".green().bold(), workspace.root.display()),
                Err(failure) => failures.push(failure),
            }
        }
    }

    for workspace in &repo_discovery.workspaces {
        match configure_workspace_target(workspace) {
            Ok(target) => println!("{} {}", "Configured workspace target".green().bold(), target.display()),
            Err(error) => failures.push(Failure::repo(workspace.repo.clone(), error.to_string())),
        }
    }

    for repo in &repo_discovery.repos {
        println!("{} {}", "Removing quarantine metadata in".blue().bold(), repo.display());
    }

    for cleanup in quarantine::clean(&repo_discovery.repos, opts.jobs) {
        failures.extend(cleanup.failures.into_iter().map(|failure| Failure::Quarantine {
            repo: cleanup.repo.clone(),
            failure,
        }));
    }

    summarize_repos(repo_discovery.repos.len(), &failures)
}

fn validate_directory(directory: &Path) -> rootcause::Result<PathBuf> {
    let metadata =
        std::fs::symlink_metadata(directory).attach_with(|| format!("directory not found: {}", directory.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(
            report!("refusing to use a symbolic-link directory").attach(format!("path={}", directory.display()))
        );
    }
    if !metadata.is_dir() {
        return Err(report!("not a directory").attach(format!("path={}", directory.display())));
    }
    Ok(std::fs::canonicalize(directory)
        .attach_with(|| format!("cannot canonicalize directory: {}", directory.display()))?)
}

fn require_command(command: &str) -> rootcause::Result<()> {
    if Command::new(command).arg("--version").output().is_err() {
        return Err(report!("required command not found").attach(format!("command={command}")));
    }
    Ok(())
}

fn collect_manifest_paths(directory: &Path) -> ManifestDiscovery {
    let mut manifests = Vec::new();
    let mut failures = Vec::new();
    collect_manifest_paths_recursive(directory, &mut manifests, &mut failures);
    ManifestDiscovery { manifests, failures }
}

fn collect_manifest_paths_recursive(directory: &Path, manifests: &mut Vec<PathBuf>, failures: &mut Vec<Failure>) {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            failures.push(Failure::Traversal {
                message: format!("Cargo manifest discovery failed below {}: {error}", directory.display()),
            });
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(Failure::Traversal {
                    message: format!("reading entry below {} failed: {error}", directory.display()),
                });
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                failures.push(Failure::Traversal {
                    message: format!("reading file type for {} failed: {error}", path.display()),
                });
                continue;
            }
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_manifest_paths_recursive(&path, manifests, failures);
        } else if file_type.is_file() && path.file_name() == Some(OsStr::new("Cargo.toml")) {
            manifests.push(path);
        }
    }
}

fn discover_repos(manifests: &[PathBuf]) -> RepoDiscovery {
    let mut manifest_count = 0_usize;
    let mut repos = Vec::new();
    let mut repo_set = HashSet::new();
    let mut workspaces = Vec::new();
    let mut workspace_set = HashSet::new();
    let mut skipped_manifests = Vec::new();
    let mut failures = Vec::new();

    for manifest in manifests {
        let Some(manifest_directory) = manifest.parent() else {
            continue;
        };
        let Some(repo) = repo_root(manifest_directory) else {
            skipped_manifests.push(manifest.clone());
            continue;
        };
        manifest_count = manifest_count.saturating_add(1);

        if repo_set.insert(repo.clone()) {
            repos.push(repo.clone());
        }
        let metadata = match cargo_metadata(manifest) {
            Ok(metadata) => metadata,
            Err(error) => {
                failures.push(Failure::repo(
                    repo,
                    format!("could not resolve Cargo metadata for {}: {error}", manifest.display()),
                ));
                continue;
            }
        };

        let root = match std::fs::canonicalize(&metadata.workspace_root) {
            Ok(root) => root,
            Err(error) => {
                failures.push(Failure::repo(
                    repo,
                    format!(
                        "could not canonicalize Cargo workspace root {}: {error}",
                        metadata.workspace_root.display()
                    ),
                ));
                continue;
            }
        };

        if workspace_set.contains(&root) {
            continue;
        }
        let workspace_manifest = root.join("Cargo.toml");
        let metadata = match cargo_metadata(&workspace_manifest) {
            Ok(metadata) => metadata,
            Err(error) => {
                failures.push(Failure::repo(
                    repo,
                    format!(
                        "could not resolve Cargo metadata for workspace {}: {error}",
                        workspace_manifest.display()
                    ),
                ));
                continue;
            }
        };

        if workspace_set.insert(root.clone()) {
            workspaces.push(Workspace {
                repo,
                root,
                target: metadata.target_directory,
            });
        }
    }

    RepoDiscovery {
        manifest_count,
        repos,
        workspaces,
        skipped_manifests,
        failures,
    }
}

fn repo_root(directory: &Path) -> Option<PathBuf> {
    let repo = GitRepo::discover(directory).ok()?;
    std::fs::canonicalize(repo.workdir()?).ok()
}

fn cargo_metadata(manifest: &Path) -> rootcause::Result<CargoMetadata> {
    let directory = manifest
        .parent()
        .ok_or_else(|| report!("Cargo manifest has no parent").attach(format!("manifest={}", manifest.display())))?;
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest)
        .current_dir(directory)
        .output()
        .attach_with(|| format!("failed to run cargo metadata for {}", manifest.display()))?;

    if !output.status.success() {
        return Err(report!("cargo metadata failed").attach(format!("manifest={}", manifest.display())));
    }

    Ok(serde_json::from_slice(&output.stdout)
        .attach_with(|| format!("invalid cargo metadata for {}", manifest.display()))?)
}

fn clean_workspaces(workspaces: &[Workspace], jobs: usize) -> Vec<Result<Workspace, Failure>> {
    let mut pending = Vec::new();
    let mut cleanups = Vec::new();

    for workspace in workspaces {
        pending.push((workspace.clone(), spawn_cargo_clean(workspace)));
        if pending.len() >= jobs {
            cleanups.push(collect_cargo_cleanup(pending.remove(0)));
        }
    }

    for cleanup in pending {
        cleanups.push(collect_cargo_cleanup(cleanup));
    }
    cleanups
}

fn spawn_cargo_clean(workspace: &Workspace) -> thread::JoinHandle<std::io::Result<bool>> {
    let root = workspace.root.clone();
    thread::spawn(move || {
        Command::new("cargo")
            .arg("clean")
            .current_dir(&root)
            .status()
            .map(|status| status.success())
    })
}

fn collect_cargo_cleanup(
    (workspace, cleanup): (Workspace, thread::JoinHandle<std::io::Result<bool>>),
) -> Result<Workspace, Failure> {
    match cleanup.join() {
        Ok(Ok(true)) => Ok(workspace),
        Ok(Ok(false)) => Err(Failure::repo(
            workspace.repo.clone(),
            format!("cargo clean failed: {}", workspace.root.display()),
        )),
        Ok(Err(error)) => Err(Failure::repo(
            workspace.repo.clone(),
            format!("could not run cargo clean for {}: {error}", workspace.root.display()),
        )),
        Err(_) => Err(Failure::repo(
            workspace.repo.clone(),
            format!("cargo clean worker panicked: {}", workspace.root.display()),
        )),
    }
}

fn configure_workspace_target(workspace: &Workspace) -> rootcause::Result<PathBuf> {
    std::fs::create_dir_all(&workspace.target).attach_with(|| {
        format!(
            "could not create workspace target directory: {}",
            workspace.target.display()
        )
    })?;
    let target = std::fs::canonicalize(&workspace.target).attach_with(|| {
        format!(
            "could not canonicalize workspace target directory: {}",
            workspace.target.display()
        )
    })?;
    if target == workspace.root || !target.starts_with(&workspace.root) {
        return Err(
            report!("Cargo target directory is outside its workspace").attach(format!(
                "workspace={} target={}",
                workspace.root.display(),
                target.display()
            )),
        );
    }

    let output = Command::new("tmutil")
        .arg("isexcluded")
        .arg(&target)
        .output()
        .attach_with(|| format!("could not inspect Time Machine exclusion: {}", target.display()))?;
    if !output.status.success() {
        return Err(report!("Time Machine exclusion inspection failed").attach(format!("target={}", target.display())));
    }

    let state = String::from_utf8_lossy(&output.stdout);
    match (state.contains("[Included]"), state.contains("[Excluded]")) {
        (true, false) => {
            let status = Command::new("tmutil")
                .arg("addexclusion")
                .arg(&target)
                .status()
                .attach_with(|| format!("could not add Time Machine exclusion: {}", target.display()))?;
            if !status.success() {
                return Err(
                    report!("could not add Time Machine exclusion").attach(format!("target={}", target.display()))
                );
            }
        }
        (false, true) => {}
        _ => {
            return Err(report!("unrecognized Time Machine exclusion state")
                .attach(format!("target={} output={state:?}", target.display())));
        }
    }

    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(target.join(".metadata_never_index"))
        .attach_with(|| format!("could not create Spotlight sentinel in {}", target.display()))?;

    Ok(target)
}

fn summarize_repos(processed: usize, failures: &[Failure]) -> rootcause::Result<()> {
    for failure in failures {
        match failure {
            Failure::Repo { repo, message } => {
                eprintln!("{} {message}: {}", "Error".red().bold(), repo.display());
            }
            Failure::Quarantine { repo, failure } => {
                eprintln!("{} {failure}: {}", "Error".red().bold(), repo.display());
            }
            Failure::Traversal { message } => eprintln!("{} {message}", "Error".red().bold()),
        }
    }
    if failures
        .iter()
        .any(|failure| matches!(failure, Failure::Traversal { .. }))
    {
        eprintln!("{} repo traversal was incomplete", "Error".red().bold());
    }

    let failed_repositories = failures
        .iter()
        .filter_map(|failure| match failure {
            Failure::Repo { repo, .. } | Failure::Quarantine { repo, .. } => Some(repo),
            Failure::Traversal { .. } => None,
        })
        .collect::<HashSet<_>>()
        .len();

    if processed == 0 {
        println!("{}", "No Rust repos found".yellow().bold());
    } else if failures.is_empty() {
        println!("{} {processed} Rust repo(s)", "Cleaned".green().bold());
    } else {
        eprintln!(
            "{} {} of {processed} Rust repo(s) failed",
            "Error".red().bold(),
            failed_repositories
        );
    }

    if failures.is_empty() {
        return Ok(());
    }
    Err(report!("Rust repo maintenance failed"))
}
