use std::fs;
use std::path::Path;
use std::process::Command;

const INPUTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots");

#[rstest::rstest]
#[case::item_ordering("item-ordering")]
#[case::call_qualification("call-qualification")]
#[case::item_qualification("item-qualification")]
#[case::import_aliasing("import-aliasing")]
#[case::path_anchoring("path-anchoring")]
fn test_rsl_when_rule_runs_rewrites_the_file_and_stays_stable(#[case] rule: &str) {
    let directory = tempfile::tempdir().unwrap();
    let name = rule.replace('-', "_");
    let filename = format!("{name}.rs");
    let path = directory.path().join(&filename);
    fs::copy(Path::new(INPUTS).join(filename), &path).unwrap();

    if matches!(rule, "path-anchoring" | "call-qualification") {
        let crate_root = if rule == "call-qualification" {
            fs::write(
                directory.path().join("lib.rs"),
                r#"
                    #[path = "call_qualification.rs"]
                    mod fixture;
                "#,
            )
            .unwrap();

            "lib"
        } else {
            &name
        };

        fs::write(
            directory.path().join("Cargo.toml"),
            format!(
                r#"
                    [package]
                    name = "rsl-fixture"
                    version = "0.1.0"
                    edition = "2024"
                    [lib]
                    path = "{crate_root}.rs"
                    [workspace]
                "#
            ),
        )
        .unwrap();
    }

    let mut command = Command::new(env!("CARGO_BIN_EXE_frs"));
    command.args(["rsl", "--rules", rule]).arg(&path);

    let output = command.output().unwrap();

    assert!(output.status.success(), "{output:?}");

    let rewritten = fs::read_to_string(&path).unwrap();
    insta::with_settings!({ prepend_module_to_snapshot => false }, {
        insta::assert_snapshot!(name, &rewritten);
    });

    let output = command.output().unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), rewritten);
}

#[rstest::rstest]
#[case::item_ordering("item-ordering")]
#[case::call_qualification("call-qualification")]
#[case::item_qualification("item-qualification")]
#[case::import_aliasing("import-aliasing")]
#[case::path_anchoring("path-anchoring")]
fn test_rsl_when_reporting_each_rule_preserves_the_source(#[case] rule: &str) {
    let directory = tempfile::tempdir().unwrap();
    let filename = format!("{}.rs", rule.replace('-', "_"));
    let path = directory.path().join(&filename);
    fs::copy(Path::new(INPUTS).join(filename), &path).unwrap();
    let original = fs::read(&path).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .args(["rsl", "--mode", "report", "--verbose", "--rules", rule])
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!output.stdout.is_empty(), "{output:?}");
    assert!(String::from_utf8(output.stdout).unwrap().contains(&format!("[{rule}")));
}

#[test]
fn test_rsl_when_a_file_has_multiple_modules_only_anchors_agreed_paths() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        r#"
            [package]
            name = "shared-module-fixture"
            version = "0.1.0"
            edition = "2024"
            [lib]
            path = "lib.rs"
            [workspace]
        "#,
    )
    .unwrap();
    let root = directory.path().join("lib.rs");
    let root_source = r#"
            #[path = "shared.rs"]
            mod first;
            #[path = "shared.rs"]
            mod second;
        "#;
    fs::write(&root, root_source).unwrap();

    let path = directory.path().join("shared.rs");
    let original = r"
            use super::Root;
            use self::Child;

            mod nested {
                use super::Child;
            }
        ";
    fs::write(&path, original).unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_frs"));
    command
        .args(["rsl", "--verbose", "--rules", "path-anchoring"])
        .arg(&path);

    let report = command.args(["--mode", "report"]).output().unwrap();

    assert!(report.status.success(), "{report:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    let findings = String::from_utf8(report.stdout).unwrap();

    assert_eq!(findings.matches("available [path-anchoring]").count(), 1);
    assert_eq!(
        findings.matches("path-anchoring/conflicting-module-contexts").count(),
        2
    );

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .args(["rsl", "--rules", "path-anchoring"])
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");

    let expected = original.replacen("super::Root", "crate::Root", 1);

    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    assert_eq!(fs::read_to_string(&root).unwrap(), root_source);
}

#[rstest::rstest]
#[case::multiple_occurrences(
    r#"
        #[path = "shared.rs"]
        mod first;
        #[path = "shared.rs"]
        mod second;
    "#,
    "crate::first::helper"
)]
#[case::opaque_module_macro(
    r#"
        macro_rules! make_modules {
            () => {};
        }
        make_modules!();
        #[path = "shared.rs"]
        mod first;
    "#,
    "helper"
)]
fn test_rsl_when_macros_or_shared_files_are_present_uses_visible_module_identity(
    #[case] root_source: &str,
    #[case] expected_call: &str,
) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        r#"
            [package]
            name = "uncertain-module-fixture"
            version = "0.1.0"
            edition = "2024"
            [lib]
            path = "lib.rs"
            [workspace]
        "#,
    )
    .unwrap();
    fs::write(directory.path().join("lib.rs"), root_source).unwrap();
    fs::write(directory.path().join("child.rs"), "").unwrap();

    let path = directory.path().join("shared.rs");
    let original = r"
            mod child;

            pub(crate) fn helper() {}

            fn run() {
                crate::first::helper();
            }
        ";
    fs::write(&path, original).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .args(["rsl", "--rules", "call-qualification"])
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let expected = original.replace("crate::first::helper", expected_call);

    assert_eq!(fs::read_to_string(path).unwrap(), expected);
}

#[test]
fn test_rsl_when_reporting_an_available_fix_preserves_a_read_only_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.rs");
    let original = "fn run(_: std::path::PathBuf) {}\n";
    fs::write(&path, original).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .args(["rsl", "--mode", "report", "--verbose", "--rules", "item-qualification"])
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "{}:1:11: available [item-qualification]: replace `std::path::PathBuf` with `PathBuf`\n  \
                 Help: Run with --mode write to apply this change.\n",
            path.display(),
        ),
    );
    assert_eq!(output.stderr, Vec::<u8>::new());
}

#[rstest::rstest]
#[case::default(&[], true)]
#[case::write(&["--mode", "write"], true)]
#[case::report(&["--mode", "report"], false)]
fn test_rsl_when_not_verbose_suppresses_findings_without_changing_the_mode(
    #[case] options: &[&str],
    #[case] writes: bool,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.rs");
    let original = "fn run(_: std::path::PathBuf) { let _ = super::read(); }\n";
    fs::write(&path, original).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .arg("rsl")
        .args(options)
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.stderr, Vec::<u8>::new());
    let expected = if writes {
        "use std::path::PathBuf;\nfn run(_: PathBuf) { let _ = super::read(); }\n"
    } else {
        original
    };
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
}

#[test]
fn test_rsl_when_not_verbose_still_reports_parse_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.rs");
    let original = "fn broken(\n";
    fs::write(&path, original).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .arg("rsl")
        .arg(&path)
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert!(!output.stderr.is_empty(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[rstest::rstest]
#[case::default(&["--verbose"])]
#[case::explicit(&["--mode", "write", "--verbose"])]
fn test_rsl_when_writing_reports_only_findings_at_final_locations(#[case] options: &[&str]) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.rs");
    fs::write(
        &path,
        "fn hidden() { let _ = super::read(); }\npub fn entry(_: std::path::PathBuf) {}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .arg("rsl")
        .args(options)
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let rewritten = fs::read_to_string(&path).unwrap();
    assert_eq!(
        rewritten,
        "use std::path::PathBuf;\npub fn entry(_: PathBuf) {}\nfn hidden() { let _ = super::read(); }\n"
    );
    assert_eq!(
        String::from_utf8(output.stdout.clone()).unwrap(),
        format!(
            "{}:3:23: skipped [path-anchoring/parent-module-unknown]: replace parent-module reference `super`\n  \
                 Reason: The required crate or parent-module context is outside this file.\n  \
                 Help: Use the correct crate:: path after checking the surrounding modules.\n",
            path.display(),
        ),
    );

    let report = Command::new(env!("CARGO_BIN_EXE_frs"))
        .args(["rsl", "--mode", "report", "--verbose"])
        .arg(&path)
        .output()
        .unwrap();

    assert!(report.status.success(), "{report:?}");
    assert_eq!(report.stdout, output.stdout);
    assert_eq!(fs::read_to_string(&path).unwrap(), rewritten);
    assert_eq!(output.stderr, Vec::<u8>::new());
    assert_eq!(report.stderr, Vec::<u8>::new());
}

#[rstest::rstest]
#[case::unknown(&["--mode", "unknown"])]
#[case::missing(&["--mode"])]
#[case::repeated(&["--mode", "write", "--mode", "report"])]
fn test_rsl_when_the_mode_is_invalid_rejects_it_without_writing(#[case] options: &[&str]) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.rs");
    let original = "fn run(_: std::path::PathBuf) {}\n";
    fs::write(&path, original).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .arg("rsl")
        .arg(&path)
        .args(options)
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(path).unwrap(), original);
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_ne!(output.stderr, Vec::<u8>::new());
}

#[rstest::rstest]
#[case::name_conflict(
    "item-qualification",
    r"
            struct PathBuf;

            fn run(_: std::path::PathBuf) {}
        ",
    "<file>:4:23: skipped [item-qualification/name-conflict]: replace `std::path::PathBuf` with `PathBuf`\n  \
         Reason: The name `PathBuf` may refer to another item or local binding.\n  \
         Help: Keep the existing spelling, or rename the conflicting item.\n"
)]
#[case::comments(
    "item-qualification",
    r"
            fn run(_: std /* keep */ ::path::PathBuf) {}
        ",
    "<file>:2:23: skipped [item-qualification/comments]: replace `std::path::PathBuf` with `PathBuf`\n  \
         Reason: The change could remove comments inside the affected code.\n  \
         Help: Move or preserve the comments before making this change manually.\n"
)]
#[case::unknown_imports(
    "item-qualification",
    r"
            use other::*;

            fn run(_: std::path::PathBuf) {}
        ",
    "<file>:4:23: skipped [item-qualification/unknown-imports]: replace `std::path::PathBuf` with `PathBuf`\n  \
         Reason: An import leaves names unresolved, or this path depends on a conditional import.\n  \
         Help: Use explicit imports so the names available in this module can be checked.\n"
)]
#[case::conditional_item(
    "item-qualification",
    r"
            #[cfg(test)]
            fn run(_: std::path::PathBuf) {}
        ",
    "<file>:3:23: skipped [item-qualification/conditional-code]: replace `std::path::PathBuf` with `PathBuf`\n  \
         Reason: Conditional code may need different imports in different builds.\n  \
         Help: Check the paths and imports in each supported configuration before changing them together.\n"
)]
#[case::trait_import(
    "item-qualification",
    r"
            fn run<T: external::Inspect>() {}
        ",
    "<file>:2:23: skipped [item-qualification/trait-import]: replace `external::Inspect` with `Inspect`\n  \
         Reason: Importing this trait could change how existing method calls are resolved.\n  \
         Help: Keep the qualified trait path to preserve method lookup in this scope.\n"
)]
#[case::unknown_item_kind(
    "item-qualification",
    r"
            fn run() {
                external::Factory::make();
            }
        ",
    "<file>:3:17: skipped [item-qualification/unknown-item-kind]: replace `external::Factory` with `Factory`\n  \
         Reason: This associated path could refer to a trait or a type.\n  \
         Help: Keep the qualified path unless the item is known to be a type or is already imported.\n"
)]
#[case::exported_alias(
    "import-aliasing",
    r"
            pub use std::path::PathBuf as Buffer;
        ",
    "<file>:2:43: skipped [import-aliasing/exported-alias]: replace alias `Buffer` with `PathBuf`\n  \
         Reason: Other code may depend on the name exported by this import.\n  \
         Help: Keep this exported name unless its callers can also be updated.\n"
)]
#[case::module_import_capture(
    "call-qualification",
    r#"
            fn other(value: &str) {
                str::len(value);
            }

            fn run() {
                std::str::from_utf8(b"x");
            }
        "#,
    "<file>:7:17: skipped [call-qualification/import-capture]: replace `std::str::from_utf8` with `str::from_utf8`\n  \
         Reason: Adding an import named `str` could change existing references.\n  \
         Help: Check the existing references before introducing this module import.\n"
)]
#[case::module_import_external_references(
    "call-qualification",
    r#"
            mod child;

            fn run() {
                std::fs::read_to_string("x");
            }
        "#,
    "<file>:5:17: skipped [call-qualification/external-modules]: replace `std::fs::read_to_string` with `fs::read_to_string`\n  \
         Reason: A child module in another file may observe the new module import.\n  \
         Help: Check child-module references before introducing this import.\n"
)]
fn test_rsl_when_a_change_is_blocked_reports_its_exact_cause(
    #[case] rule: &str,
    #[case] source: &str,
    #[case] expected: &str,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.rs");
    fs::write(&path, source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .args(["rsl", "--mode", "report", "--verbose", "--rules", rule])
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    let reported = String::from_utf8(output.stdout)
        .unwrap()
        .replace(path.to_str().unwrap(), "<file>");
    assert_eq!(reported, expected);
    assert_eq!(output.stderr, Vec::<u8>::new());
}

#[test]
fn test_rsl_when_reporting_multiple_rules_inspects_the_same_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.rs");
    let source = "use std::fs::read_to_string as read;\nfn run() { read(\"x\"); }\n";
    fs::write(&path, source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_frs"))
        .args([
            "rsl",
            "--mode",
            "report",
            "--verbose",
            "--rules",
            "import-aliasing,call-qualification",
        ])
        .arg(&path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    let reported = String::from_utf8(output.stdout)
        .unwrap()
        .replace(path.to_str().unwrap(), "<file>");
    assert_eq!(
        reported,
        "<file>:1:32: available [import-aliasing]: replace alias `read` with `read_to_string`\n  \
             Help: Run with --mode write to apply this change.\n\n\
             <file>:2:12: available [call-qualification]: replace `read` with `fs::read_to_string`\n  \
             Help: Run with --mode write to apply this change.\n",
    );
}
