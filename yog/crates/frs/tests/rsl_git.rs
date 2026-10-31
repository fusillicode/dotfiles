use std::fs;
use std::path::Path;
use std::process::Command;

use git2::Repository;
use git2::Signature;

const ORIGINAL: &str = "fn run(_: std::path::PathBuf) {}\n";
const REWRITTEN: &str = "use std::path::PathBuf;\nfn run(_: PathBuf) {}\n";

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
#[case::default_rules(&[])]
#[case::selected_rule(&["--rules", "item-qualification"])]
fn test_rsl_when_git_selects_all_uncommitted_files_and_preserves_the_index(#[case] options: &[&str]) {
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
            ("staged-rename.rs", ORIGINAL),
            ("staged-delete.rs", ORIGINAL),
            (".gitignore", "ignored.rs\nignored-dir/\n"),
        ],
    )
    .unwrap();
    fs::write(root.join("staged.rs"), ORIGINAL).unwrap();
    fs::write(root.join("partial.rs"), "fn staged() {}\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("staged.rs")).unwrap();
    index.add_path(Path::new("partial.rs")).unwrap();
    fs::write(root.join("added.rs"), ORIGINAL).unwrap();
    index.add_path(Path::new("added.rs")).unwrap();
    fs::rename(root.join("staged-rename.rs"), root.join("staged-new-name.rs")).unwrap();
    index.remove_path(Path::new("staged-rename.rs")).unwrap();
    index.add_path(Path::new("staged-new-name.rs")).unwrap();
    fs::remove_file(root.join("staged-delete.rs")).unwrap();
    index.remove_path(Path::new("staged-delete.rs")).unwrap();
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
        .arg("rsl")
        .args(options)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    for file in [
        "modified.rs",
        "staged.rs",
        "partial.rs",
        "added.rs",
        "staged-new-name.rs",
        "new name.rs",
        "nested/untracked.rs",
    ] {
        assert_eq!(fs::read_to_string(root.join(file)).unwrap(), REWRITTEN, "{file}");
    }
    assert_eq!(fs::read_to_string(root.join("clean.rs")).unwrap(), ORIGINAL);
    for file in ["ignored.rs", "ignored-dir/input.rs", "other.txt"] {
        assert_eq!(fs::read_to_string(root.join(file)).unwrap(), "invalid Rust", "{file}");
    }
    assert!(!root.join("deleted.rs").exists());
    assert!(!root.join("renamed.rs").exists());
    assert!(!root.join("staged-rename.rs").exists());
    assert!(!root.join("staged-delete.rs").exists());
    assert_eq!(fs::read(repository.path().join("index")).unwrap(), index_before);
}

#[rstest::rstest]
#[case::quiet_untracked(&[], false, false)]
#[case::verbose_untracked(&["--verbose"], true, false)]
#[case::quiet_staged(&[], false, true)]
#[case::verbose_staged(&["--verbose"], true, true)]
fn test_rsl_when_git_report_preserves_files(#[case] options: &[&str], #[case] reports: bool, #[case] staged: bool) {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path()).unwrap();
    let path = directory.path().join("input.rs");
    fs::write(&path, ORIGINAL).unwrap();
    if staged {
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("input.rs")).unwrap();
        index.write().unwrap();
    }
    let index_before = fs::read(repository.path().join("index")).ok();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl", "--mode", "report", "--rules", "item-qualification"])
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
    assert_eq!(fs::read(repository.path().join("index")).ok(), index_before);
}

#[rstest::rstest]
#[case::clean(false)]
#[case::non_rust(true)]
fn test_rsl_when_git_has_no_matching_files_succeeds_silently(#[case] non_rust: bool) {
    let directory = tempfile::tempdir().unwrap();
    repository_with_files(directory.path(), &[("clean.rs", ORIGINAL)]).unwrap();
    if non_rust {
        fs::write(directory.path().join("notes.txt"), "notes").unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl", "--verbose"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    assert_eq!(fs::read_to_string(directory.path().join("clean.rs")).unwrap(), ORIGINAL);
}

#[rstest::rstest]
#[case::bare_path(&["git"], &["git"])]
#[case::bare_path_first(&["git", "input.rs"], &["git", "input.rs"])]
#[case::bare_path_last(&["input.rs", "git"], &["git", "input.rs"])]
#[case::separator_after_git(&["git", "--", "input.rs"], &["git", "input.rs"])]
#[case::after_separator(&["--", "git"], &["git"])]
#[case::relative_path(&["./git"], &["git"])]
#[case::file_list(&["--", "git", "input.rs"], &["git", "input.rs"])]
#[case::mixed_separator(&["input.rs", "--", "git"], &["git", "input.rs"])]
fn test_rsl_when_git_is_a_literal_path_rewrites_only_explicit_files(
    #[case] targets: &[&str],
    #[case] rewritten: &[&str],
) {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path()).unwrap();
    for file in ["git", "input.rs", "other.rs"] {
        fs::write(directory.path().join(file), ORIGINAL).unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl", "--rules", "item-qualification"])
        .args(targets)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    for file in ["git", "input.rs", "other.rs"] {
        let expected = if rewritten.contains(&file) { REWRITTEN } else { ORIGINAL };
        assert_eq!(
            fs::read_to_string(directory.path().join(file)).unwrap(),
            expected,
            "{file}"
        );
    }
}

#[test]
fn test_rsl_when_git_flag_is_supplied_rejects_it_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("input.rs"), ORIGINAL).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl", "--git"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown rsl option"), "{stderr}");
    assert_eq!(fs::read_to_string(directory.path().join("input.rs")).unwrap(), ORIGINAL);
}

#[rstest::rstest]
#[case::outside_repository(false)]
#[case::bare_repository(true)]
fn test_rsl_when_git_has_no_worktree_reports_an_error(#[case] bare: bool) {
    let directory = tempfile::tempdir().unwrap();
    if bare {
        Repository::init_bare(directory.path()).unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(directory.path())
        .args(["rsl"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let expected = if bare {
        "Git selection requires a working tree"
    } else {
        "could not discover Git repository"
    };
    assert!(stderr.contains(expected), "{stderr}");
}

#[test]
fn test_rsl_when_git_file_is_conflicted_preserves_it() {
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
        .args(["rsl"])
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
fn test_rsl_when_git_runs_in_a_linked_worktree_rewrites_only_that_worktree() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("main");
    let repository = repository_with_files(&root, &[("input.rs", ORIGINAL)]).unwrap();
    let linked = directory.path().join("linked");
    repository.worktree("linked", &linked, None).unwrap();
    fs::write(linked.join("new.rs"), ORIGINAL).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(&linked)
        .args(["rsl", "--rules", "item-qualification"])
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
#[rstest::rstest]
#[case::untracked(false)]
#[case::staged(true)]
fn test_rsl_when_git_names_are_unusual_preserves_paths_and_skips_symlinks(#[case] staged: bool) {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let repository = Repository::init(root).unwrap();
    let names = ["--git.rs", "space and\nnewline.rs", "città.rs"];
    for name in &names {
        fs::write(root.join(name), ORIGINAL).unwrap();
    }
    let external = tempfile::tempdir().unwrap();
    let target = external.path().join("target.rs");
    fs::write(&target, ORIGINAL).unwrap();
    symlink(&target, root.join("link.rs")).unwrap();
    if staged {
        let mut index = repository.index().unwrap();
        for name in names.iter().chain(std::iter::once(&"link.rs")) {
            index.add_path(Path::new(name)).unwrap();
        }
        index.write().unwrap();
    }
    let index_before = fs::read(repository.path().join("index")).ok();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .current_dir(root)
        .args(["rsl", "--rules", "item-qualification"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    for name in names {
        assert_eq!(fs::read_to_string(root.join(name)).unwrap(), REWRITTEN);
    }
    assert_eq!(fs::read_to_string(target).unwrap(), ORIGINAL);
    assert_eq!(fs::read(repository.path().join("index")).ok(), index_before);
    assert!(
        fs::symlink_metadata(root.join("link.rs"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
