#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

const BINS: &[&str] = &["idt", "yghfl", "yhfp", "oe", "catl", "gcu", "vpg", "try", "fkr"];
const LIBS: &[(&str, &str)] = &[("librua.dylib", "rua.so")];

/// Evoke yog 🍝 👀
///
/// Formats, lints, builds, and links yog bins.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut args = utils::system::get_args();

    let is_debug = drop_element(&mut args, "--debug");
    let bins_path = args.first().cloned().map_or_else(
        || utils::system::build_home_path(&[".local", "bin"]),
        |supplied_bins_path| Ok(PathBuf::from(supplied_bins_path)),
    )?;
    let target_path = args.get(1).cloned().map_or_else(
        || {
            std::env::var("CARGO_MANIFEST_DIR").map(|cargo_manifest_dir| {
                let mut x = PathBuf::from(cargo_manifest_dir);
                remove_last_n_dirs(&mut x, 2);
                x.join("target")
            })
        },
        |x| Ok(PathBuf::from(x)),
    )?;

    let (target_location, build_profile) = if is_debug {
        ("debug", None)
    } else {
        ("release", Some("--release"))
    };
    let target_path = target_path.join(target_location);

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
        symlink_bin(&bins_path, bin, &target_path)?;
    }

    for (source_name, target_name) in LIBS {
        rename_lib(&target_path, source_name, target_name)?
    }

    Ok(())
}

fn remove_last_n_dirs(path: &mut PathBuf, n: usize) {
    for _ in 0..n {
        if !path.pop() {
            return;
        }
    }
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

fn symlink_bin(bins_path: &Path, bin: &str, target_path: &Path) -> color_eyre::Result<()> {
    let bin_path = bins_path.join(bin);
    utils::system::rm_f(&bin_path)?;
    let target_bin_path = target_path.join(bin);
    std::os::unix::fs::symlink(&target_bin_path, &bin_path)?;
    println!("✅ Symlinked bin {target_bin_path:?} to {bin_path:?}");
    Ok(())
}

fn rename_lib(target_path: &Path, source_name: &str, target_name: &str) -> color_eyre::Result<()> {
    let source_lib_path = target_path.join(source_name);
    let target_lib_path = target_path.join(target_name);
    std::fs::rename(&source_lib_path, &target_lib_path)?;
    println!("✅ Renamed lib {source_lib_path:?} to {target_lib_path:?}");
    Ok(())
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
