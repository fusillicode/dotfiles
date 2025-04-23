#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

use utils::cmd::silent_cmd;

/// Install yog.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut args = utils::system::get_args();

    let is_debug = drop_element(&mut args, "--debug");
    let bins_path = args
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{}/.local/bin", std::env::var("HOME").unwrap()));
    let mut target_path = args.get(1).cloned().unwrap_or_else(|| {
        format!(
            "{}/target",
            remove_last_n_dirs(std::env::var("CARGO_MANIFEST_DIR").unwrap().as_str(), 2)
        )
    });

    let (target_location, build_profile) = if is_debug {
        ("debug", None)
    } else {
        ("release", Some("--release"))
    };
    target_path.push('/');
    target_path.push_str(target_location);

    silent_cmd("cargo").args(["fmt"]).status()?.exit_ok()?;
    silent_cmd("cargo")
        .args([
            "clippy",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ])
        .status()?
        .exit_ok()?;
    silent_cmd("cargo")
        .args([Some("build"), build_profile].into_iter().flatten())
        .status()?
        .exit_ok()?;

    for bin in [
        "idt", "yghfl", "yhfp", "oe", "catl", "gcu", "vpg", "try", "fkr",
    ] {
        let bin_path = format!("{bins_path}/{bin}");
        rm_f(&bin_path)?;
        std::os::unix::fs::symlink(format!("{target_path}/{bin}"), &bin_path)?;
    }
    std::fs::rename(
        format!("{target_path}/librua.dylib"),
        format!("{target_path}/rua.so"),
    )?;

    Ok(())
}

fn remove_last_n_dirs(path: &str, n: usize) -> String {
    let mut path = PathBuf::from(path);
    for _ in 0..n {
        if !path.pop() {
            return path.to_string_lossy().to_string();
        }
    }
    path.to_string_lossy().to_string()
}

fn rm_f<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    std::fs::remove_file(path).or_else(|error| {
        if std::io::ErrorKind::NotFound == error.kind() {
            return Ok(());
        }
        Err(error)
    })
}

fn drop_element<T, U: ?Sized>(vec: &mut Vec<T>, target: &U) -> bool
where
    T: PartialEq<U>,
{
    if let Some(idx) = vec.iter().position(|x| x == target) {
        vec.swap_remove(idx);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop_element_returns_true_and_removes_the_element_from_the_vec() {
        let mut input = vec![42, 7];
        assert!(drop_element(&mut input, &7));
        assert_eq!(vec![42], input);
    }

    #[test]
    fn test_drop_element_returns_false_and_does_nothing_to_a_non_empty_vec() {
        let mut input = vec![42, 7];
        assert!(!drop_element(&mut input, &3));
        assert_eq!(vec![42, 7], input);
    }

    #[test]
    fn test_drop_element_returns_false_and_does_nothing_to_an_empty_vec() {
        let mut input: Vec<usize> = vec![];
        assert!(!drop_element(&mut input, &3));
        assert!(input.is_empty());
    }
}
