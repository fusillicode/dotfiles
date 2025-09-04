#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre::Context;
use color_eyre::owo_colors::OwoColorize as _;

/// List of binary names that should be symlinked after building.
const BINS: &[&str] = &["idt", "yghfl", "yhfp", "oe", "catl", "gcu", "vpg", "try", "fkr"];
/// List of library files that need to be renamed after building, mapping (`source_name`, `target_name`).
const LIBS: &[(&str, &str)] = &[("libnvrim.dylib", "nvrim.so")];
const BINS_DEFAULT_PATH: &[&str] = &[".local", "bin"];
const NVIM_LIBS_DEFAULT_PATH: &[&str] = &[".config", "nvim", "lua"];

/// Automates build workflow: formats, lints, builds, and deploys yog binaries.
///
/// # Arguments
///
/// * `--debug` - Build in debug mode, skip clippy
/// * `bins_path` - Bin directory for symlinks (default: ~/.local/bin)
/// * `target_path` - Cargo target directory (default: project root target/)
///
/// # Examples
///
/// ```bash
/// evoke
/// evoke --debug
/// evoke /custom/bin/path
/// ```
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut args = utils::system::get_args();

    let is_debug = drop_element(&mut args, "--debug");
    let bins_path = args.first().cloned().map_or_else(
        || utils::system::build_home_path(BINS_DEFAULT_PATH),
        |supplied_bins_path| Ok(PathBuf::from(supplied_bins_path)),
    )?;
    let nvim_libs_path = args.get(2).cloned().map_or_else(
        || utils::system::build_home_path(NVIM_LIBS_DEFAULT_PATH),
        |supplied_nvim_libs_path| Ok(PathBuf::from(supplied_nvim_libs_path)),
    )?;
    let cargo_target_path = args.get(1).cloned().map_or_else(
        || {
            std::env::var("CARGO_MANIFEST_DIR").map(|cargo_manifest_dir| {
                let mut x = PathBuf::from(cargo_manifest_dir);
                remove_last_n_dirs(&mut x, 2);
                x.join("target")
            })
        },
        |x| Ok(PathBuf::from(x)),
    )?;

    let (cargo_target_location, build_profile) = if is_debug {
        (cargo_target_path.join("debug"), None)
    } else {
        (cargo_target_path.join("release"), Some("--release"))
    };

    utils::cmd::silent_cmd("cargo").args(["fmt"]).status()?.exit_ok()?;

    // Skip clippy if debugging
    if !is_debug {
        utils::cmd::silent_cmd("cargo")
            .args(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
            .status()?
            .exit_ok()?;
    }

    utils::cmd::silent_cmd("cargo")
        .args([Some("build"), build_profile].into_iter().flatten())
        .status()?
        .exit_ok()?;

    for bin in BINS {
        cp(&cargo_target_location.join(bin), &bins_path.join(bin))?;
    }

    for (source_lib_name, target_lib_name) in LIBS {
        cp(
            &cargo_target_location.join(source_lib_name),
            &nvim_libs_path.join(target_lib_name),
        )?;
    }

    Ok(())
}

/// Removes the last `n` directories from a [`PathBuf`].
fn remove_last_n_dirs(path: &mut PathBuf, n: usize) {
    for _ in 0..n {
        if !path.pop() {
            return;
        }
    }
}

/// Removes the first occurrence of an element from a vector.
/// Returns `true` if found and removed, `false` otherwise.
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

/// Copies the supplied path to the target and prints to stdout the desired message.
fn cp(from: &Path, to: &Path) -> color_eyre::Result<()> {
    std::fs::copy(from, to).with_context(|| format!("from {}, to {}", from.display(), to.display()))?;
    println!("{} {} to {}", "Copied".green().bold(), from.display(), to.display(),);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_element_returns_true_and_removes_the_element_from_the_vec() {
        let mut input = vec![42, 7];
        assert!(drop_element(&mut input, &7));
        assert_eq!(vec![42], input);
    }

    #[test]
    fn drop_element_returns_false_and_does_nothing_to_a_non_empty_vec() {
        let mut input = vec![42, 7];
        assert!(!drop_element(&mut input, &3));
        assert_eq!(vec![42, 7], input);
    }

    #[test]
    fn drop_element_returns_false_and_does_nothing_to_an_empty_vec() {
        let mut input: Vec<usize> = vec![];
        assert!(!drop_element(&mut input, &3));
        assert!(input.is_empty());
    }
}
