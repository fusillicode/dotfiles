//! Discover Cargo target roots once per workspace, without building or loading dependencies.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::report;
use serde::Deserialize;

#[derive(Default)]
pub(super) struct CrateRoots {
    pub(super) roots: Vec<PathBuf>,
    pub(super) manifests: Vec<PathBuf>,
}

pub(super) fn discover(paths: &[PathBuf]) -> rootcause::Result<CrateRoots> {
    let mut manifests = HashSet::new();
    let mut roots = Vec::new();

    for path in paths {
        let manifest = path
            .ancestors()
            .skip(1)
            .map(|directory| directory.join("Cargo.toml"))
            .find(|manifest| manifest.is_file());

        let Some(manifest) = manifest else {
            continue;
        };

        if !manifests.insert(manifest.clone()) {
            continue;
        }

        let metadata = metadata(&manifest)?;
        manifests.insert(metadata.workspace_root.join("Cargo.toml"));

        for package in metadata.packages {
            manifests.insert(package.manifest_path);
            roots.extend(package.targets.into_iter().map(|target| target.src_path));
        }
    }

    roots.sort();
    roots.dedup();

    Ok(CrateRoots {
        roots,
        manifests: manifests.into_iter().collect(),
    })
}

fn metadata(manifest: &Path) -> rootcause::Result<Metadata> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest)
        .current_dir(manifest.parent().unwrap_or_else(|| Path::new(".")))
        .output()
        .map_err(|error| report!("could not discover Cargo targets").attach(error))?;

    if !output.status.success() {
        return Err(report!("could not discover Cargo targets")
            .attach(format!("manifest={}", manifest.display()))
            .attach(String::from_utf8_lossy(&output.stderr).into_owned()));
    }

    serde_json::from_slice(&output.stdout).map_err(|error| report!("invalid Cargo target metadata").attach(error))
}

#[derive(Deserialize)]
struct Metadata {
    workspace_root: PathBuf,
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    manifest_path: PathBuf,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    src_path: PathBuf,
}
