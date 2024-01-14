#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! dirs = "5.0"
//! duct = "0.13.7"
//! ```

use duct::cmd;
use std::fs::File;
use std::process::Command;

fn main() {
    let home = dirs::home_dir().unwrap();
    let dev_tools_dir = home.to_path_buf().join(".dev-tools");
    let bin_dir = home.to_path_buf().join(".local/bin");

    std::fs::create_dir_all(&dev_tools_dir).unwrap();
    std::fs::create_dir_all(&bin_dir).unwrap();

    if cmd!("gh", "auth", "status").run().is_err() {
        cmd!("gh", "auth", "login").run().unwrap();
    }

    let latest_release = get_latest_release("tekumara/typos-vscode");

    let file = File::create(format!("{}/rust-analyzer", bin_dir.display())).unwrap();
    cmd!("curl", "-SL", "https://github.com/rust-lang/rust-analyzer/releases/download/nightly/rust-analyzer-aarch64-apple-darwin.gz")
        .pipe(cmd!("gunzip", "-c", "-")).stdout_file(file).run().unwrap();

    let file = File::create(format!("{}/taplo", bin_dir.display())).unwrap();
    cmd!(
        "curl",
        "-SL",
        "https://github.com/tamasfe/taplo/releases/latest/download/taplo-full-darwin-aarch64.gz"
    )
    .pipe(cmd!("gunzip", "-c", "-"))
    .stdout_file(file)
    .run()
    .unwrap();

    let latest_release = &get_latest_release("hashicorp/terraform-ls")[1..];
    cmd!(
        "curl", 
        "-SL", 
        format!("https://releases.hashicorp.com/terraform-ls/{}/terraform-ls_{latest_release}_darwin_arm64.zip", latest_release)
    )
    .pipe(cmd!("tar", "-xz", "-C", format!("{}", bin_dir.display()))).run().unwrap();

    // cmd!("chmod", "+x", format!("{}/*", bin_dir.display()))
    //     .run()
    //     .unwrap();
}

fn get_latest_release(repo: &str) -> String {
    cmd!(
        "gh",
        "api",
        &format!("repos/{repo}/releases/latest"),
        "--jq=.tag_name",
    )
    .read()
    .unwrap()
}
