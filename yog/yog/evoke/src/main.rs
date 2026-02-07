//! Format, lint, build, and deploy workspace binaries and Nvim libs.
//!
//! # Errors
//! - Cargo commands or file copy operations fail.
#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

use owo_colors::OwoColorize;
use ytil_sys::cli::Args;

/// List of binaries that should be copied after building.
/// NOTE: if a new binary is added this list must be updated!
const BINS: &[&str] = &[
    "idt", "catl", "fkr", "gch", "gcu", "ghl", "oe", "rmr", "strgci", "tec", "try", "vpg", "yghfl", "yhfp",
];
/// List of library files that need to be renamed after building, mapping (`source_name`, `target_name`).
const LIBS: &[(&str, &str)] = &[("libnvrim.dylib", "nvrim.so")];
/// Path segments for the default binaries install dir.
const BINS_DEFAULT_PATH: &[&str] = &[".local", "bin"];
/// Path segments for the Nvim libs install dir.
const NVIM_LIBS_DEFAULT_PATH: &[&str] = &[".config", "nvim", "lua"];

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

/// Copies a built binary or library from `from` to `to` using
/// [`ytil_sys::file::atomic_cp`] and prints an "Installed" status line.
///
/// # Errors
/// - [`ytil_sys::file::atomic_cp`] fails to copy.
/// - The final rename or write cannot be performed.
fn cp(from: &Path, to: &Path) -> rootcause::Result<()> {
    ytil_sys::file::atomic_cp(from, to)?;
    println!("{} {} to {}", "Copied".green().bold(), from.display(), to.display());
    Ok(())
}

/// Format, lint, build, and deploy workspace binaries and Nvim libs.
fn main() -> rootcause::Result<()> {
    let mut args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let is_debug = drop_element(&mut args, "--debug");
    let bins_path = args.first().cloned().map_or_else(
        || ytil_sys::dir::build_home_path(BINS_DEFAULT_PATH),
        |supplied_bins_path| Ok(PathBuf::from(supplied_bins_path)),
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
    let nvim_libs_path = args.get(2).cloned().map_or_else(
        || ytil_sys::dir::build_home_path(NVIM_LIBS_DEFAULT_PATH),
        |supplied_nvim_libs_path| Ok(PathBuf::from(supplied_nvim_libs_path)),
    )?;

    let (cargo_target_location, build_profile) = if is_debug {
        (cargo_target_path.join("debug"), None)
    } else {
        (cargo_target_path.join("release"), Some("--release"))
    };

    ytil_cmd::silent_cmd("cargo").args(["fmt"]).status()?.exit_ok()?;

    // Skip clippy if debugging
    if !is_debug {
        ytil_cmd::silent_cmd("cargo")
            .args(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
            .status()?
            .exit_ok()?;
    }

    ytil_cmd::silent_cmd("cargo")
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn drop_element_returns_true_and_removes_the_element_from_the_vec() {
        let mut input = vec![42, 7];
        assert!(drop_element(&mut input, &7));
        assert_eq!(input, vec![42]);
    }

    #[test]
    fn drop_element_returns_false_and_does_nothing_to_a_non_empty_vec() {
        let mut input = vec![42, 7];
        assert!(!drop_element(&mut input, &3));
        assert_eq!(input, vec![42, 7]);
    }

    #[test]
    fn drop_element_returns_false_and_does_nothing_to_an_empty_vec() {
        let mut input: Vec<usize> = vec![];
        assert!(!drop_element(&mut input, &3));
        assert!(input.is_empty());
    }

    #[rstest]
    #[case::no_dirs_removed(PathBuf::from("/home/user/docs"), 0, PathBuf::from("/home/user/docs"))]
    #[case::remove_one_dir(PathBuf::from("/home/user/docs"), 1, PathBuf::from("/home/user"))]
    #[case::remove_more_than_exist(PathBuf::from("/home/user"), 5, PathBuf::from("/"))]
    #[case::root_path(PathBuf::from("/"), 1, PathBuf::from("/"))]
    #[case::empty_path(PathBuf::new(), 1, PathBuf::new())]
    fn remove_last_n_dirs_works(#[case] mut initial: PathBuf, #[case] n: usize, #[case] expected: PathBuf) {
        remove_last_n_dirs(&mut initial, n);
        pretty_assertions::assert_eq!(initial, expected);
    }
}
