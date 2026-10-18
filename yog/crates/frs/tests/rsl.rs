#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    const INPUTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots");

    #[rstest::rstest]
    #[case::ordering("ordering-rule")]
    #[case::unqualified_calls("unqualified-call")]
    #[case::overqualified_calls("overqualified-call")]
    #[case::qualified_items("qualified-item")]
    #[case::aliases("aliased-import")]
    #[case::relative_paths("relative-path")]
    fn test_rsl_when_rule_runs_rewrites_the_file_and_stays_stable(#[case] rule: &str) {
        let directory = tempfile::tempdir().unwrap();
        let name = rule.replace('-', "_");
        let filename = format!("{name}.rs");
        let path = directory.path().join(&filename);
        fs::copy(Path::new(INPUTS).join(filename), &path).unwrap();

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
        assert_eq!(fs::read_to_string(path).unwrap(), rewritten);
    }

    #[rstest::rstest]
    #[case::ordering("ordering-rule")]
    #[case::unqualified_calls("unqualified-call")]
    #[case::overqualified_calls("overqualified-call")]
    #[case::qualified_items("qualified-item")]
    #[case::aliases("aliased-import")]
    #[case::relative_paths("relative-path")]
    fn test_rsl_when_reporting_each_rule_preserves_the_source(#[case] rule: &str) {
        let directory = tempfile::tempdir().unwrap();
        let filename = format!("{}.rs", rule.replace('-', "_"));
        let path = directory.path().join(&filename);
        fs::copy(Path::new(INPUTS).join(filename), &path).unwrap();
        let original = fs::read(&path).unwrap();

        let output = Command::new(env!("CARGO_BIN_EXE_frs"))
            .args(["rsl", "--mode", "report", "--rules", rule])
            .arg(&path)
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!output.stdout.is_empty(), "{output:?}");
        assert!(String::from_utf8(output.stdout).unwrap().contains(&format!("[{rule}")));
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
            .args(["rsl", "--mode", "report", "--rules", "qualified-item"])
            .arg(&path)
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "{}:1:11: available [qualified-item]: replace `std::path::PathBuf` with `PathBuf`\n  \
                 Help: Run with --mode write to apply this change.\n",
                path.display(),
            ),
        );
        assert_eq!(output.stderr, Vec::<u8>::new());
    }

    #[rstest::rstest]
    #[case::default(&[])]
    #[case::explicit(&["--mode", "write"])]
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
                "{}:3:23: skipped [relative-path/parent-module-unknown]: replace parent-module reference `super`\n  \
                 Reason: The required crate or parent-module context is outside this file.\n  \
                 Help: Use the correct crate:: path after checking the surrounding modules.\n",
                path.display(),
            ),
        );

        let report = Command::new(env!("CARGO_BIN_EXE_frs"))
            .args(["rsl", "--mode", "report"])
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
        "qualified-item",
        "struct PathBuf;\nfn run(_: std::path::PathBuf) {}\n",
        "<file>:2:11: skipped [qualified-item/name-conflict]: replace `std::path::PathBuf` with `PathBuf`\n  \
         Reason: The name `PathBuf` may refer to another item or local binding.\n  \
         Help: Keep the existing spelling, or rename the conflicting item.\n"
    )]
    #[case::comments(
        "qualified-item",
        "fn run(_: std /* keep */ ::path::PathBuf) {}\n",
        "<file>:1:11: skipped [qualified-item/comments]: replace `std::path::PathBuf` with `PathBuf`\n  \
         Reason: The change could remove comments inside the affected code.\n  \
         Help: Move or preserve the comments before making this change manually.\n"
    )]
    #[case::unknown_imports(
        "qualified-item",
        "use other::*;\nfn run(_: std::path::PathBuf) {}\n",
        "<file>:2:11: skipped [qualified-item/unknown-imports]: replace `std::path::PathBuf` with `PathBuf`\n  \
         Reason: A wildcard, attributed, or absolute import prevents checking all available names.\n  \
         Help: Use explicit imports so the names available in this module can be checked.\n"
    )]
    #[case::exported_alias(
        "aliased-import",
        "pub use std::path::PathBuf as Buffer;\n",
        "<file>:1:31: skipped [aliased-import/exported-alias]: replace alias `Buffer` with `PathBuf`\n  \
         Reason: Other code may depend on the name exported by this import.\n  \
         Help: Keep this exported name unless its callers can also be updated.\n"
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
            .args(["rsl", "--mode", "report", "--rules", rule])
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
            .args(["rsl", "--mode", "report", "--rules", "aliased-import,unqualified-call"])
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
            "<file>:1:32: available [aliased-import]: replace alias `read` with `read_to_string`\n  \
             Help: Run with --mode write to apply this change.\n\n\
             <file>:2:12: available [unqualified-call]: replace `read` with `fs::read_to_string`\n  \
             Help: Run with --mode write to apply this change.\n",
        );
    }
}
