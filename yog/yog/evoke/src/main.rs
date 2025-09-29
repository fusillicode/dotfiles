#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

use color_eyre::owo_colors::OwoColorize;

/// List of binary names that should be symlinked after building.
const BINS: &[&str] = &["idt", "catl", "fkr", "gch", "gcu", "oe", "try", "vpg", "yghfl", "yhfp"];
/// List of library files that need to be renamed after building, mapping (`source_name`, `target_name`).
const LIBS: &[(&str, &str)] = &[("libnvrim.dylib", "nvrim.so")];
/// Path segments for the default binaries install dir.
const BINS_DEFAULT_PATH: &[&str] = &[".local", "bin"];
/// Path segments for the Nvim libs install dir.
const NVIM_LIBS_DEFAULT_PATH: &[&str] = &[".config", "nvim", "lua"];

/// Formats, lints, builds, and deploys yog binaries and its Neovim libs.
///
/// # Usage
/// `evoke [--debug] [bins_path] [cargo_target_path] [nvim_libs_path]`
///
/// `--debug` may appear anywhere; it is removed before positional argument parsing.
///
/// # Arguments
/// * `--debug` – Use debug profile, skip clippy and copy from `target/debug`.
/// * `bins_path` – Destination for binaries, defaulting to `$HOME/.local/bin`.
/// * `cargo_target_path` – Cargo target root containing `debug/` & `release/`, defaulting to project root `target/`.
/// * `nvim_libs_path` – Destination for renamed Neovim libs (e.g. `nvrim.so`), defaulting to `$HOME/.config/nvim/lua`.
///
/// Omit trailing path arguments to accept defaults.
///
/// # Errors
///
/// Returns an error if:
/// - A required environment variable is missing or invalid Unicode.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut args = ytil_system::get_args();

    let is_debug = drop_element(&mut args, "--debug");
    let bins_path = args.first().cloned().map_or_else(
        || ytil_system::build_home_path(BINS_DEFAULT_PATH),
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
        || ytil_system::build_home_path(NVIM_LIBS_DEFAULT_PATH),
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
/// [`ytil_system::atomic_cp`] and prints an "Installed" status line.
///
/// # Errors
///
/// Returns an error if:
/// - [`ytil_system::atomic_cp`] fails to copy.
/// - The final rename or write cannot be performed.
fn cp(from: &Path, to: &Path) -> color_eyre::Result<()> {
    ytil_system::atomic_cp(from, to)?;
    println!("{} {} to {}", "Copied".green().bold(), from.display(), to.display());
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
