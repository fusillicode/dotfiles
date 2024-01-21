use std::fs::File;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;

pub fn run<'a>(mut args: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let home = PathBuf::from(std::env::var("HOME")?);
    let dev_tools_dir = home.to_path_buf().join(".dev-tools");
    let bin_dir = home.to_path_buf().join(".local/bin");

    std::fs::create_dir_all(dev_tools_dir)?;
    std::fs::create_dir_all(bin_dir)?;

    authenticate_to_github()?;

    // let latest_release = get_latest_release("tekumara/typos-vscode");
    //
    // let file = File::create(format!("{}/rust-analyzer", bin_dir.display())).unwrap();
    // cmd!("curl", "-SL", "https://github.com/rust-lang/rust-analyzer/releases/download/nightly/rust-analyzer-aarch64-apple-darwin.gz")
    //     .pipe(cmd!("gunzip", "-c", "-")).stdout_file(file).run().unwrap();
    //
    // let file = File::create(format!("{}/taplo", bin_dir.display())).unwrap();
    // cmd!(
    //     "curl",
    //     "-SL",
    //     "https://github.com/tamasfe/taplo/releases/latest/download/taplo-full-darwin-aarch64.gz"
    // )
    // .pipe(cmd!("gunzip", "-c", "-"))
    // .stdout_file(file)
    // .run()
    // .unwrap();
    //
    // let latest_release = &get_latest_release("hashicorp/terraform-ls")[1..];
    // cmd!(
    //     "curl",
    //     "-SL",
    //     format!("https://releases.hashicorp.com/terraform-ls/{}/terraform-ls_{latest_release}_darwin_arm64.zip", latest_release)
    // )
    // .pipe(cmd!("tar", "-xz", "-C", format!("{}", bin_dir.display()))).run().unwrap();
    //
    // cmd!("chmod", "+x", format!("{}/*", bin_dir.display()))
    //     .run()
    //     .unwrap();
    println!("{}", std::env::var("HOME").unwrap());
    todo!()
}

fn authenticate_to_github() -> anyhow::Result<()> {
    if Command::new("gh")
        .args(["auth", "status"])
        .status()?
        .success()
    {
        return Ok(());
    }

    // Spawning a new shell because `gh` should block until the user is authenticated
    Command::new("sh")
        .args(["-c", "gh auth login"])
        .spawn()?
        .wait()?;

    Ok(())
}

// fn get_latest_release(repo: &str) -> String {
//     cmd!(
//         "gh",
//         "api",
//         &format!("repos/{repo}/releases/latest"),
//         "--jq=.tag_name",
//     )
//     .read()
//     .unwrap()
// }
