use std::path::Path;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;

pub fn run() -> rootcause::Result<()> {
    let zshrc = std::env::var("HOME")
        .context("error missing HOME environment variable")
        .map(|home| Path::new(&home).join(".zshrc"))?;

    install_zsh_wrapper_at(&zshrc)?;
    println!("{} gbm in {}", "Patched".green().bold(), zshrc.display());

    Ok(())
}

fn install_zsh_wrapper_at(path: &Path) -> rootcause::Result<bool> {
    let content = std::fs::read_to_string(path)
        .context("error reading zshrc")
        .attach_with(|| format!("path={}", path.display()))?;

    if content.lines().any(|line| line.trim() == super::ZSHRC_INSTALL_LINE) {
        return Ok(false);
    }

    let mut updated = content;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(super::ZSHRC_INSTALL_LINE);
    updated.push('\n');

    std::fs::write(path, updated)
        .context("error installing zshrc")
        .attach_with(|| format!("path={}", path.display()))?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_install_zsh_wrapper_at_appends_guarded_line_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let zshrc = dir.path().join(".zshrc");
        std::fs::write(&zshrc, "source ~/.zshrc.local\n").unwrap();

        assert_that!(install_zsh_wrapper_at(&zshrc), ok(eq(true)));
        let first = std::fs::read_to_string(&zshrc).unwrap();
        assert_that!(install_zsh_wrapper_at(&zshrc), ok(eq(false)));
        let second = std::fs::read_to_string(&zshrc).unwrap();

        assert_that!(first, eq(second));
        assert_that!(
            first,
            eq(format!("source ~/.zshrc.local\n{}\n", super::super::ZSHRC_INSTALL_LINE))
        );
        assert_that!(first.matches(super::super::ZSHRC_INSTALL_LINE).count(), eq(1));
    }

    #[test]
    fn test_install_zsh_wrapper_at_fails_when_zshrc_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let zshrc = dir.path().join(".zshrc");

        assert_that!(
            install_zsh_wrapper_at(&zshrc),
            err(displays_as(contains_substring("error reading zshrc")))
        );
    }
}
