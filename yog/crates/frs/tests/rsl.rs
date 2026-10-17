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
}
