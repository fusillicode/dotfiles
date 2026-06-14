use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::prelude::ResultExt;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Metadata {
    packages: Vec<Package>,
}

impl Metadata {
    pub fn read(workspace_root: &Path) -> rootcause::Result<Self> {
        let mut command = Command::new("cargo");
        command
            .args(["metadata", "--format-version=1", "--no-deps"])
            .current_dir(workspace_root);
        let output = command.output().context("failed to spawn cargo metadata")?;
        output.status.exit_ok().context("cargo metadata failed")?;
        Self::from_slice(&output.stdout)
    }

    pub fn native_audit_bin_paths(&self) -> Vec<PathBuf> {
        let mut paths = self
            .packages
            .iter()
            .flat_map(|package| package.targets.iter())
            .filter(|target| target.is_default_bin())
            .map(|target| Path::new("./target/release").join(&target.name))
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    pub fn native_bin_package_names(&self) -> Vec<&str> {
        let mut package_names = self
            .packages
            .iter()
            .filter(|package| package.targets.iter().any(Target::is_default_bin))
            .map(|package| package.name.as_str())
            .collect::<Vec<_>>();
        package_names.sort_unstable();
        package_names
    }

    fn from_slice(metadata: &[u8]) -> rootcause::Result<Self> {
        Ok(serde_json::from_slice::<Self>(metadata).context("failed to parse cargo metadata")?)
    }
}

#[derive(Deserialize)]
struct Package {
    name: String,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    kind: Vec<String>,
    name: String,
    #[serde(default, rename = "required-features")]
    required_features: Vec<String>,
}

impl Target {
    fn is_default_bin(&self) -> bool {
        self.kind.iter().any(|kind| kind == "bin") && self.required_features.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_audit_bin_paths_uses_native_bin_targets() {
        assert2::assert!(let Ok(metadata) = Metadata::from_slice(metadata_fixture()));

        pretty_assertions::assert_eq!(
            metadata.native_audit_bin_paths(),
            vec![
                PathBuf::from("./target/release/evoke"),
                PathBuf::from("./target/release/fixture-tool"),
                PathBuf::from("./target/release/helper-cli"),
            ]
        );
    }

    #[test]
    fn test_native_bin_package_names_uses_native_bin_targets() {
        assert2::assert!(let Ok(metadata) = Metadata::from_slice(metadata_fixture()));

        pretty_assertions::assert_eq!(
            metadata.native_bin_package_names(),
            vec!["evoke", "fixture-tool", "helper-cli"]
        );
    }

    fn metadata_fixture() -> &'static [u8] {
        br#"
            {
              "packages": [
                {
                  "dependencies": [],
                  "manifest_path": "/repo/yog/evoke/Cargo.toml",
                  "name": "evoke",
                  "targets": [
                    { "kind": ["bin"], "name": "evoke" },
                    { "kind": ["lib"], "name": "evoke" }
                  ]
                },
                {
                  "dependencies": [],
                  "manifest_path": "/repo/yog/helper-cli/Cargo.toml",
                  "name": "helper-cli",
                  "targets": [
                    { "kind": ["bin"], "name": "helper-cli" }
                  ]
                },
                {
                  "dependencies": [],
                  "manifest_path": "/repo/yog/feature-bin/Cargo.toml",
                  "name": "feature-bin",
                  "targets": [
                    {
                      "kind": ["bin"],
                      "name": "feature-bin",
                      "required-features": ["cli"]
                    }
                  ]
                },
                {
                  "dependencies": [
                    { "kind": "dev", "name": "fixture-dev-dep" }
                  ],
                  "manifest_path": "/repo/yog/fixture-tool/Cargo.toml",
                  "name": "fixture-tool",
                  "targets": [
                    { "kind": ["bin"], "name": "fixture-tool" }
                  ]
                }
              ]
            }
        "#
    }
}
