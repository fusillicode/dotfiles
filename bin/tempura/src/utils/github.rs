use std::process::Command;

pub fn log_into_github() -> anyhow::Result<()> {
    if Command::new("gh")
        .args(["auth", "status"])
        .status()?
        .success()
    {
        return Ok(());
    }

    Ok(Command::new("sh")
        .args(["-c", "gh auth login"])
        .status()?
        .exit_ok()?)
}

pub fn get_latest_release(repo: &str) -> anyhow::Result<String> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{repo}/releases/latest"),
            "--jq=.tag_name",
        ])
        .output()?;

    output.status.exit_ok()?;

    Ok(std::str::from_utf8(&output.stdout)?.trim().into())
}
