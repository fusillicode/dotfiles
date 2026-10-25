//! Generate vendor-specific subagent files from the generic subagents in `<src_dir>/subagents/`.
//!
//! Read the `.md` files in that dir. Each file has YAML `name`, `description`,
//! optional `vendors` settings, and the subagent instructions after the YAML:
//!
//! ```text
//! ---
//! name: reviewer
//! description: Reviews changes.
//! vendors:
//!   claude:
//!     model: sonnet
//! ---
//! Review changes and report findings with file locations.
//! ```
//!
//! Names start with `a-z` or `0-9` and contain only `a-z`, `0-9`, `-`, or `_`.
//! The description and instructions cannot be blank.
//!
//! Use only the settings for the supplied vendor. Generate TOML for Codex and
//! Markdown with YAML for the other vendors. Codex uses `developer_instructions`
//! for the instructions. `OpenCode` uses the filename for the subagent name and
//! requires `mode: subagent`. Vendor settings cannot replace the shared fields.
//!
//! Write `agents/<name>.toml` under Codex's config dir and `agents/<name>.md`
//! under the other vendors' config dirs.
//!
//! Generate each subagent independently. A file error prints a warning and does not
//! stop other subagents.
//!
//! For duplicate subagent names, keep the first valid subagent in filename order.
//! Overwrite different generated regular files; leave identical files
//! unchanged. Preserve conflicting symlinks and dirs as conflicts.

use std::fmt::Formatter;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use bbtree::unsync::BBTreeMap;
use bbtree::unsync::BBTreeSet;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;
use serde::de::MapAccess;
use serde::de::Visitor;
use serde_json::Map;
use serde_json::Value;
use ytil_agents::agent::Agent as Vendor;

use crate::cmds::sync_harness::common;
use crate::cmds::sync_harness::common::SyncOutcome;

pub(super) fn sync_subagents(src_dir: &Path, home: &Path, vendors: &[Vendor]) -> Vec<SyncOutcome> {
    let (subagent_files, mut outcomes) = collect_subagent_files(src_dir);
    let mut subagent_names = BBTreeSet::new();

    for subagent_file in subagent_files {
        let result = prepare_subagent_files(&subagent_file, home, vendors, &mut subagent_names)
            .attach_with(|| format!("cannot sync subagent | path={}", subagent_file.display()));
        let vendor_subagent_files = match result {
            Ok(files) => files,
            Err(error) => {
                outcomes.push(SyncOutcome::SubagentSkipped { error });
                continue;
            }
        };

        for file in vendor_subagent_files {
            let result = file
                .write_file()
                .attach_with(|| format!("cannot sync subagent | path={}", subagent_file.display()));
            match result {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => {
                    outcomes.push(SyncOutcome::SubagentSkipped { error });
                    break;
                }
            }
        }
    }

    outcomes
}

struct VendorSubagentFile {
    destination: PathBuf,
    contents: String,
}

impl VendorSubagentFile {
    fn write_file(&self) -> rootcause::Result<SyncOutcome> {
        let dir = self
            .destination
            .parent()
            .ok_or_else(|| report!("generated subagent has no dir"))?;

        if let Some(outcome) = common::prepare_dir(dir, "subagent", None)? {
            return Ok(outcome);
        }

        common::write_generated_file(&self.contents, &self.destination, "subagent")
    }
}

fn collect_subagent_files(src_dir: &Path) -> (Vec<PathBuf>, Vec<SyncOutcome>) {
    let mut outcomes = Vec::new();
    let entries = match fs::read_dir(src_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return (Vec::new(), outcomes),
        Err(error) => {
            let error = report!(error).attach(format!("cannot read subagents | path={}", src_dir.display()));
            outcomes.push(SyncOutcome::SubagentSkipped { error: error.into() });
            return (Vec::new(), outcomes);
        }
    };
    let mut subagent_files = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                let error = report!(error).attach(format!("cannot read subagent file | path={}", src_dir.display()));
                outcomes.push(SyncOutcome::SubagentSkipped { error: error.into() });
                continue;
            }
        };
        let path = entry.path();

        if path.extension().is_some_and(|extension| extension == "md") {
            subagent_files.push(path);
        }
    }

    subagent_files.sort();
    (subagent_files, outcomes)
}

fn prepare_subagent_files(
    path: &Path,
    home: &Path,
    vendors: &[Vendor],
    subagent_names: &mut BBTreeSet<String>,
) -> rootcause::Result<Vec<VendorSubagentFile>> {
    let contents = fs::read_to_string(path).attach("cannot read subagent")?;
    let generic_subagent = GenericSubagent::parse(&contents).attach("invalid subagent definition")?;

    if subagent_names.contains(&generic_subagent.name) {
        return Err(report!("duplicate subagent name").attach(format!("name={}", generic_subagent.name)));
    }

    let mut vendor_subagent_files = Vec::new();

    for vendor in vendors {
        let vendor_subagent_file = generic_subagent
            .generate_for_vendor(*vendor, home)
            .attach_with(|| format!("cannot generate vendor subagent file | vendor={}", vendor.name()))?;
        vendor_subagent_files.push(vendor_subagent_file);
    }

    subagent_names.insert(generic_subagent.name);

    Ok(vendor_subagent_files)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubagentFrontmatter {
    name: String,
    description: String,
    #[serde(default, deserialize_with = "deserialize_vendors")]
    vendors: BBTreeMap<String, Map<String, Value>>,
}

fn deserialize_vendors<'de, D>(deserializer: D) -> Result<BBTreeMap<String, Map<String, Value>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct VendorsVisitor;

    impl<'de> Visitor<'de> for VendorsVisitor {
        type Value = BBTreeMap<String, Map<String, Value>>;

        fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a map")
        }

        fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut vendors = BBTreeMap::new();

            while let Some((vendor, settings)) = access.next_entry()? {
                vendors.insert(vendor, settings);
            }

            Ok(vendors)
        }
    }

    deserializer.deserialize_map(VendorsVisitor)
}

struct GenericSubagent {
    name: String,
    description: String,
    instructions: String,
    vendors: BBTreeMap<Vendor, Map<String, Value>>,
}

impl GenericSubagent {
    fn parse(contents: &str) -> rootcause::Result<Self> {
        let mut lines = contents.split_inclusive('\n');

        if lines.next().map(str::trim_end) != Some("---") {
            return Err(report!("subagent must start with YAML frontmatter delimited by ---"));
        }

        let mut yaml = String::new();
        let mut closed = false;

        for line in lines.by_ref() {
            if line.trim_end() == "---" {
                closed = true;
                break;
            }

            yaml.push_str(line);
        }

        if !closed {
            return Err(report!("subagent frontmatter is missing its closing ---"));
        }

        let frontmatter: SubagentFrontmatter = serde_saphyr::from_str(&yaml)?;
        let instructions = lines.collect::<String>();

        Self::validate(frontmatter, instructions)
    }

    fn validate(frontmatter: SubagentFrontmatter, instructions: String) -> rootcause::Result<Self> {
        let name = &frontmatter.name;
        let starts_with_alphanumeric = name.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric);

        if !starts_with_alphanumeric
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_'))
        {
            return Err(report!(
                "subagent name must start with a lowercase letter or digit and contain only a-z, 0-9, - or _"
            ));
        }

        if frontmatter.description.trim().is_empty() || instructions.trim().is_empty() {
            return Err(report!("subagent description and instructions must not be empty"));
        }

        let mut vendors = BBTreeMap::new();

        for (name, settings) in frontmatter.vendors {
            let vendor = Vendor::from_name(&name)?;
            validate_vendor_settings(vendor, &settings)?;
            vendors.insert(vendor, settings);
        }

        Ok(Self {
            name: frontmatter.name,
            description: frontmatter.description,
            instructions,
            vendors,
        })
    }

    fn generate_for_vendor(&self, vendor: Vendor, home: &Path) -> rootcause::Result<VendorSubagentFile> {
        let (extension, contents) = self.generate_vendor_contents(vendor)?;
        let root = common::vendor_config_dir(home, vendor);

        Ok(VendorSubagentFile {
            destination: root.join("agents").join(format!("{}.{extension}", self.name)),
            contents,
        })
    }

    fn generate_vendor_contents(&self, vendor: Vendor) -> rootcause::Result<(&'static str, String)> {
        let mut settings = self.vendor_settings(vendor);

        match vendor {
            Vendor::Codex => {
                settings.insert(
                    "developer_instructions".to_owned(),
                    Value::String(self.instructions.clone()),
                );

                Ok(("toml", toml::to_string_pretty(&settings)?))
            }
            Vendor::Claude | Vendor::Cursor | Vendor::Gemini | Vendor::Opencode => {
                let yaml = serde_saphyr::to_string(&settings)?;

                Ok(("md", format!("---\n{yaml}---\n{}", self.instructions)))
            }
        }
    }

    fn vendor_settings(&self, vendor: Vendor) -> Map<String, Value> {
        let mut settings = self.vendors.get(&vendor).cloned().unwrap_or_default();
        settings.insert("description".to_owned(), Value::String(self.description.clone()));

        match vendor {
            Vendor::Opencode => {
                settings.insert("mode".to_owned(), Value::String("subagent".to_owned()));
            }
            Vendor::Claude | Vendor::Codex | Vendor::Cursor | Vendor::Gemini => {
                settings.insert("name".to_owned(), Value::String(self.name.clone()));
            }
        }

        settings
    }
}

fn validate_vendor_settings(vendor: Vendor, settings: &Map<String, Value>) -> rootcause::Result<()> {
    for key in settings.keys() {
        if matches!(
            key.as_str(),
            "name" | "description" | "developer_instructions" | "vendors"
        ) || reserved_fields_for_vendor(vendor).contains(&key.as_str())
        {
            return Err(report!("vendor settings cannot replace shared subagent fields")
                .attach(format!("vendor={} field={key}", vendor.name())));
        }
    }

    Ok(())
}

const fn reserved_fields_for_vendor(vendor: Vendor) -> &'static [&'static str] {
    match vendor {
        Vendor::Opencode => &["mode"],
        Vendor::Claude | Vendor::Codex | Vendor::Cursor | Vendor::Gemini => &[],
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Deserializer;
    use test_that::prelude::*;

    use super::*;

    #[rstest::rstest]
    #[case::empty("{}", &[])]
    #[case::unordered(
        r#"{"gemini":{"model":"second"},"claude":{"model":"first"}}"#,
        &[("claude", "first"), ("gemini", "second")]
    )]
    #[case::duplicate(
        r#"{"claude":{"model":"old"},"claude":{"model":"new"}}"#,
        &[("claude", "new")]
    )]
    fn test_deserialize_vendors_when_map_is_valid_preserves_sorted_entries_and_last_values(
        #[case] json: &str,
        #[case] expected: &[(&str, &str)],
    ) {
        let mut deserializer = Deserializer::from_str(json);
        let expected_entries = expected
            .iter()
            .map(|(name, model)| {
                (
                    (*name).to_owned(),
                    Map::from_iter([("model".to_owned(), Value::String((*model).to_owned()))]),
                )
            })
            .collect::<Vec<_>>();

        let vendors = deserialize_vendors(&mut deserializer).expect("valid vendor settings");

        assert_eq!(vendors.into_iter().collect::<Vec<_>>(), expected_entries);
    }

    #[rstest::rstest]
    #[case::sequence("[]")]
    #[case::null("null")]
    #[case::non_map_settings(r#"{"claude":42}"#)]
    fn test_deserialize_vendors_when_map_is_invalid_rejects_it(#[case] json: &str) {
        let mut deserializer = Deserializer::from_str(json);

        let actual = deserialize_vendors(&mut deserializer);

        assert_that!(actual, err(anything()));
    }
}
