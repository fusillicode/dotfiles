use std::fs;
use std::path::Path;
use std::process::Command;

use git2::Repository;
use git2::Signature;

const ORIGINAL: &str = "fn run(_: std::path::PathBuf) {}\n";
const REWRITTEN: &str = "use std::path::PathBuf;\nfn run(_: PathBuf) {}\n";

#[test]
fn test_rsl_when_unstaged_selects_worktree_changes_and_preserves_the_index() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let repository = repository_with_files(
        root,
        &[
            ("modified.rs", "fn old() {}\n"),
            ("staged.rs", "fn old() {}\n"),
            ("partial.rs", "fn old() {}\n"),
            ("clean.rs", ORIGINAL),
            ("deleted.rs", ORIGINAL),
            ("renamed.rs", ORIGINAL),
            (".gitignore", "ignored.rs\nignored-dir/\n"),
        ],
    )
    .unwrap();
    fs::write(root.join("staged.rs"), ORIGINAL).unwrap();
    fs::write(root.join("partial.rs"), "fn staged() {}\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("staged.rs")).unwrap();
    index.add_path(Path::new("partial.rs")).unwrap();
    index.write().unwrap();
    let index_before = fs::read(repository.path().join("index")).unwrap();
    fs::write(root.join("modified.rs"), ORIGINAL).unwrap();
    fs::write(root.join("partial.rs"), ORIGINAL).unwrap();
    fs::remove_file(root.join("deleted.rs")).unwrap();
    fs::rename(root.join("renamed.rs"), root.join("new name.rs")).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("nested/untracked.rs"), ORIGINAL).unwrap();
    fs::write(root.join("ignored.rs"), "invalid Rust").unwrap();
    fs::create_dir(root.join("ignored-dir")).unwrap();
    fs::write(root.join("ignored-dir/input.rs"), "invalid Rust").unwrap();
    fs::write(root.join("other.txt"), "invalid Rust").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(root.join("nested"))
        .args(["rsl", "--unstaged", "--rules", "item-qualification"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    for file in ["modified.rs", "partial.rs", "new name.rs", "nested/untracked.rs"] {
        assert_eq!(fs::read_to_string(root.join(file)).unwrap(), REWRITTEN, "{file}");
    }
    for file in ["staged.rs", "clean.rs"] {
        assert_eq!(fs::read_to_string(root.join(file)).unwrap(), ORIGINAL, "{file}");
    }
    for file in ["ignored.rs", "ignored-dir/input.rs", "other.txt"] {
        assert_eq!(fs::read_to_string(root.join(file)).unwrap(), "invalid Rust", "{file}");
    }
    assert!(!root.join("deleted.rs").exists());
    assert!(!root.join("renamed.rs").exists());
    assert_eq!(fs::read(repository.path().join("index")).unwrap(), index_before);
}

fn repository_with_files(root: &Path, files: &[(&str, &str)]) -> rootcause::Result<Repository> {
    let repository = Repository::init(root)?;
    let mut index = repository.index()?;
    for (path, contents) in files {
        fs::write(root.join(path), contents)?;
        index.add_path(Path::new(path))?;
    }
    index.write()?;
    let tree_id = index.write_tree()?;
    let signature = Signature::now("RSL fixture", "rsl@example.invalid")?;
    {
        let tree = repository.find_tree(tree_id)?;
        repository.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])?;
    }
    Ok(repository)
}

#[rstest::rstest]
#[case::quiet(&[], false)]
#[case::verbose(&["--verbose"], true)]
fn test_rsl_when_unstaged_report_preserves_files(#[case] options: &[&str], #[case] reports: bool) {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path()).unwrap();
    let path = directory.path().join("input.rs");
    fs::write(&path, ORIGINAL).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl", "--unstaged", "--mode", "report", "--rules", "item-qualification"])
        .args(options)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(path).unwrap(), ORIGINAL);
    if reports {
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("available [item-qualification]"), "{stdout}");
    } else {
        assert_eq!(output.stdout, Vec::<u8>::new());
    }
    assert_eq!(output.stderr, Vec::<u8>::new());
    assert!(!repository.path().join("index").exists());
}

#[rstest::rstest]
#[case::clean(false)]
#[case::non_rust(true)]
fn test_rsl_when_unstaged_has_no_matching_files_succeeds_silently(#[case] non_rust: bool) {
    let directory = tempfile::tempdir().unwrap();
    repository_with_files(directory.path(), &[("clean.rs", ORIGINAL)]).unwrap();
    if non_rust {
        fs::write(directory.path().join("notes.txt"), "notes").unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl", "--unstaged", "--verbose"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    assert_eq!(fs::read_to_string(directory.path().join("clean.rs")).unwrap(), ORIGINAL);
}

#[rstest::rstest]
#[case::normal(&["--unstaged", "input.rs"])]
#[case::after_separator(&["--unstaged", "--", "input.rs"])]
fn test_rsl_when_unstaged_is_combined_with_paths_rejects_without_writing(#[case] options: &[&str]) {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path()).unwrap();
    fs::write(directory.path().join("input.rs"), ORIGINAL).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .arg("rsl")
        .args(options)
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("--unstaged cannot be combined with file paths"),
        "{stderr}"
    );
    assert_eq!(fs::read_to_string(directory.path().join("input.rs")).unwrap(), ORIGINAL);
}

#[rstest::rstest]
#[case::outside_repository(false)]
#[case::bare_repository(true)]
fn test_rsl_when_unstaged_has_no_worktree_reports_an_error(#[case] bare: bool) {
    let directory = tempfile::tempdir().unwrap();
    if bare {
        Repository::init_bare(directory.path()).unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl", "--unstaged"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let expected = if bare {
        "--unstaged requires a Git working tree"
    } else {
        "could not discover Git repository for --unstaged"
    };
    assert!(stderr.contains(expected), "{stderr}");
}

#[test]
fn test_rsl_when_unstaged_file_is_conflicted_preserves_it() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository_with_files(directory.path(), &[("conflict.rs", ORIGINAL)]).unwrap();
    let mut index = repository.index().unwrap();
    let mut entry = index.get_path(Path::new("conflict.rs"), 0).unwrap();
    index.remove(Path::new("conflict.rs"), 0).unwrap();
    entry.flags |= 0x2000;
    index.add(&entry).unwrap();
    index.write().unwrap();
    let index_before = fs::read(repository.path().join("index")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl", "--unstaged"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    assert_eq!(
        fs::read_to_string(directory.path().join("conflict.rs")).unwrap(),
        ORIGINAL
    );
    assert_eq!(fs::read(repository.path().join("index")).unwrap(), index_before);
}

#[test]
fn test_rsl_when_unstaged_runs_in_a_linked_worktree_rewrites_only_that_worktree() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("main");
    let repository = repository_with_files(&root, &[("input.rs", ORIGINAL)]).unwrap();
    let linked = directory.path().join("linked");
    repository.worktree("linked", &linked, None).unwrap();
    fs::write(linked.join("new.rs"), ORIGINAL).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(&linked)
        .args(["rsl", "--unstaged", "--rules", "item-qualification"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    assert_eq!(fs::read_to_string(linked.join("new.rs")).unwrap(), REWRITTEN);
    assert_eq!(fs::read_to_string(linked.join("input.rs")).unwrap(), ORIGINAL);
    assert_eq!(fs::read_to_string(root.join("input.rs")).unwrap(), ORIGINAL);
}

#[cfg(unix)]
#[test]
fn test_rsl_when_unstaged_names_are_unusual_preserves_paths_and_skips_symlinks() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    Repository::init(root).unwrap();
    let names = ["--unstaged.rs", "space and\nnewline.rs", "città.rs"];
    for name in &names {
        fs::write(root.join(name), ORIGINAL).unwrap();
    }
    let external = tempfile::tempdir().unwrap();
    let target = external.path().join("target.rs");
    fs::write(&target, ORIGINAL).unwrap();
    symlink(&target, root.join("link.rs")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(root)
        .args(["rsl", "--unstaged", "--rules", "item-qualification"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    for name in names {
        assert_eq!(fs::read_to_string(root.join(name)).unwrap(), REWRITTEN);
    }
    assert_eq!(fs::read_to_string(target).unwrap(), ORIGINAL);
    assert!(
        fs::symlink_metadata(root.join("link.rs"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
