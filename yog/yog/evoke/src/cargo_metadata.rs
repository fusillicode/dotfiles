use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::prelude::ResultExt;
use serde::Deserialize;

/// Dependency that identifies a workspace package as a Zellij WASM plugin.
const ZELLIJ_TILE_DEPENDENCY: &str = "zellij-tile";

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
            .filter(|package| !package.is_zellij_plugin())
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
            .filter(|package| !package.is_zellij_plugin())
            .filter(|package| package.targets.iter().any(Target::is_default_bin))
            .map(|package| package.name.as_str())
            .collect::<Vec<_>>();
        package_names.sort_unstable();
        package_names
    }

    pub fn zellij_plugin_manifests(&self) -> Vec<PathBuf> {
        let mut manifests = self
            .packages
            .iter()
            .filter(|package| package.is_zellij_plugin())
            .map(|package| package.manifest_path.clone())
            .collect::<Vec<_>>();
        manifests.sort();
        manifests
    }

    pub fn zellij_plugin_package_names(&self) -> Vec<&str> {
        let mut package_names = self
            .packages
            .iter()
            .filter(|package| package.is_zellij_plugin())
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
struct Dependency {
    #[serde(default)]
    kind: Option<String>,
    name: String,
}

#[derive(Deserialize)]
struct Package {
    dependencies: Vec<Dependency>,
    manifest_path: PathBuf,
    name: String,
    targets: Vec<Target>,
}

impl Package {
    fn is_zellij_plugin(&self) -> bool {
        self.dependencies
            .iter()
            .any(|dependency| dependency.name == ZELLIJ_TILE_DEPENDENCY && dependency.kind.is_none())
    }
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
                PathBuf::from("./target/release/zj"),
            ]
        );
    }

    #[test]
    fn test_native_bin_package_names_uses_native_bin_targets() {
        assert2::assert!(let Ok(metadata) = Metadata::from_slice(metadata_fixture()));

        pretty_assertions::assert_eq!(metadata.native_bin_package_names(), vec!["evoke", "fixture-tool", "zj"]);
    }

    #[test]
    fn test_zellij_plugin_manifests_uses_zellij_tile_dependency() {
        assert2::assert!(let Ok(metadata) = Metadata::from_slice(metadata_fixture()));

        pretty_assertions::assert_eq!(
            metadata.zellij_plugin_manifests(),
            vec![
                PathBuf::from("/repo/moved/zcp/Cargo.toml"),
                PathBuf::from("/repo/yog/zj/plugins/agg/Cargo.toml"),
            ]
        );
    }

    #[test]
    fn test_zellij_plugin_package_names_uses_zellij_tile_dependency() {
        assert2::assert!(let Ok(metadata) = Metadata::from_slice(metadata_fixture()));

        pretty_assertions::assert_eq!(metadata.zellij_plugin_package_names(), vec!["agg", "zellij-copy"]);
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
                  "manifest_path": "/repo/yog/zj/cli/Cargo.toml",
                  "name": "zj",
                  "targets": [
                    { "kind": ["bin"], "name": "zj" }
                  ]
                },
                {
                  "dependencies": [
                    { "kind": null, "name": "zellij-tile" }
                  ],
                  "manifest_path": "/repo/yog/zj/plugins/agg/Cargo.toml",
                  "name": "agg",
                  "targets": [
                    { "kind": ["bin"], "name": "agg" }
                  ]
                },
                {
                  "dependencies": [
                    { "kind": null, "name": "zellij-tile" }
                  ],
                  "manifest_path": "/repo/moved/zcp/Cargo.toml",
                  "name": "zellij-copy",
                  "targets": [
                    { "kind": ["bin"], "name": "zcp" }
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
                    { "kind": "dev", "name": "zellij-tile" }
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
