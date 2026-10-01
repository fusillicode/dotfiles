use nutype::nutype;
use rootcause::report;
use serde::Deserialize;
use serde::Deserializer;

/// External JSON layout used only to seed a brand-new muxr session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalSessionLayout {
    tabs: Vec<ExternalLayoutTab>,
}

impl ExternalSessionLayout {
    /// Validate the layout seed before creating runtime panes.
    ///
    /// # Errors
    /// Returns an error when tabs or panes are empty, or command arguments lack a command.
    pub fn validate(&self) -> rootcause::Result<()> {
        if self.tabs.is_empty() {
            return Err(report!("muxr external layout has no tabs"));
        }
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            tab.validate(tab_index)?;
        }
        Ok(())
    }

    pub fn tabs(&self) -> &[ExternalLayoutTab] {
        &self.tabs
    }
}

impl<'de> Deserialize<'de> for ExternalSessionLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawExternalSessionLayout::deserialize(deserializer)?;
        let tabs = raw
            .tabs
            .into_iter()
            .enumerate()
            .map(|(index, tab)| ExternalLayoutTab::from_raw(tab, index))
            .collect::<rootcause::Result<Vec<_>>>()
            .map_err(|error| serde::de::Error::custom(format!("{error:#}")))?;
        Ok(Self { tabs })
    }
}

/// One tab in an external muxr layout seed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalLayoutTab {
    cwd: LayoutCwd,
    panes: Vec<ExternalLayoutPane>,
}

impl ExternalLayoutTab {
    pub fn cwd(&self) -> &str {
        self.cwd.as_ref()
    }

    pub fn panes(&self) -> &[ExternalLayoutPane] {
        &self.panes
    }

    fn from_raw(raw: RawExternalLayoutTab, tab_index: usize) -> rootcause::Result<Self> {
        let cwd = LayoutCwd::try_new(raw.cwd)
            .map_err(|_| report!("muxr external layout tab cwd is empty").attach(format!("tab_index={tab_index}")))?;
        let panes = raw
            .panes
            .into_iter()
            .enumerate()
            .map(|(pane_index, pane)| ExternalLayoutPane::from_raw(pane, tab_index, pane_index))
            .collect::<rootcause::Result<Vec<_>>>()?;
        Ok(Self { cwd, panes })
    }

    fn validate(&self, tab_index: usize) -> rootcause::Result<()> {
        if self.panes.is_empty() {
            return Err(report!("muxr external layout tab has no panes").attach(format!("tab_index={tab_index}")));
        }
        for (pane_index, pane) in self.panes.iter().enumerate() {
            pane.validate(tab_index, pane_index)?;
        }
        Ok(())
    }
}

/// One pane in an external muxr layout seed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalLayoutPane {
    #[serde(default)]
    args: Vec<String>,
    cmd: Option<LayoutCommand>,
}

impl ExternalLayoutPane {
    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn cmd(&self) -> Option<&str> {
        self.cmd.as_ref().map(AsRef::as_ref)
    }

    fn from_raw(raw: RawExternalLayoutPane, tab_index: usize, pane_index: usize) -> rootcause::Result<Self> {
        let cmd = raw
            .cmd
            .map(|command| {
                LayoutCommand::try_new(command).map_err(|_| {
                    report!("muxr external layout pane cmd is empty")
                        .attach(format!("tab_index={tab_index}"))
                        .attach(format!("pane_index={pane_index}"))
                })
            })
            .transpose()?;
        Ok(Self { args: raw.args, cmd })
    }

    fn validate(&self, tab_index: usize, pane_index: usize) -> rootcause::Result<()> {
        if self.cmd.is_none() && !self.args.is_empty() {
            return Err(report!("muxr external layout pane command args require cmd")
                .attach(format!("tab_index={tab_index}"))
                .attach(format!("pane_index={pane_index}")));
        }
        Ok(())
    }
}

#[nutype(
    validate(predicate = is_nonblank),
    derive(Clone, Debug, Eq, PartialEq, AsRef, Deserialize),
)]
struct LayoutCwd(String);

#[nutype(
    validate(predicate = is_nonblank),
    derive(Clone, Debug, Eq, PartialEq, AsRef, Deserialize),
)]
struct LayoutCommand(String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExternalSessionLayout {
    tabs: Vec<RawExternalLayoutTab>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExternalLayoutTab {
    cwd: String,
    panes: Vec<RawExternalLayoutPane>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExternalLayoutPane {
    #[serde(default)]
    args: Vec<String>,
    cmd: Option<String>,
}

fn is_nonblank(value: &str) -> bool {
    !value.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[rstest::rstest]
    #[case::cwd(r#"{"tabs":[{"cwd":" \t","panes":[{}]}]}"#, "tab_index=0")]
    #[case::cmd(r#"{"tabs":[{"cwd":"/tmp","panes":[{},{"cmd":" \n"}]}]}"#, "pane_index=1")]
    fn test_external_session_layout_when_scalar_is_blank_reports_location(#[case] raw: &str, #[case] location: &str) {
        assert_that!(
            serde_json::from_str::<ExternalSessionLayout>(raw),
            err(displays_as(contains_substring(location)))
        );
    }

    #[test]
    fn test_external_session_layout_when_values_have_spaces_preserves_input() -> rootcause::Result<()> {
        let layout: ExternalSessionLayout =
            serde_json::from_str(r#"{"tabs":[{"cwd":" /tmp ","panes":[{"cmd":" shell "}]}]}"#)?;
        layout.validate()?;
        let tab = layout.tabs().first().ok_or_else(|| report!("missing fixture tab"))?;
        let pane = tab.panes().first().ok_or_else(|| report!("missing fixture pane"))?;
        assert_that!(tab.cwd(), eq(" /tmp "));
        assert_that!(pane.cmd(), eq(Some(" shell ")));
        Ok(())
    }

    #[test]
    fn test_external_session_layout_when_valid_layout_is_parsed_matches_tabs() -> rootcause::Result<()> {
        let layout: ExternalSessionLayout = serde_json::from_str(
            r#"{
                "tabs": [
                    {"cwd":"/tmp/one","panes":[{}]},
                    {"cwd":"/tmp/two","panes":[{"cmd":"demo","args":["process","start"]}]}
                ]
            }"#,
        )?;

        layout.validate()?;

        let tabs = layout.tabs();
        assert_that!(tabs.len(), eq(2));
        assert_that!(tabs[0].cwd(), eq("/tmp/one"));
        assert_that!(tabs[1].cwd(), eq("/tmp/two"));

        let demo_process = tabs[1]
            .panes()
            .first()
            .ok_or_else(|| report!("expected demo process command pane"))?;
        assert_that!(demo_process.cmd(), eq(Some("demo")));
        assert_that!(demo_process.args(), eq(["process", "start"]));
        Ok(())
    }

    #[rstest::rstest]
    #[case::no_tabs(r#"{"tabs":[]}"#)]
    #[case::empty_cwd(r#"{"tabs":[{"cwd":"","panes":[{}]}]}"#)]
    #[case::no_panes(r#"{"tabs":[{"cwd":"/tmp","panes":[]}]}"#)]
    #[case::empty_cmd(r#"{"tabs":[{"cwd":"/tmp","panes":[{"cmd":"","args":[]}]}]}"#)]
    #[case::args_without_cmd(r#"{"tabs":[{"cwd":"/tmp","panes":[{"args":["server","start"]}]}]}"#)]
    fn test_external_session_layout_validate_when_required_fields_are_empty_returns_error(#[case] raw: &str) {
        let result = serde_json::from_str::<ExternalSessionLayout>(raw)
            .map_err(|error| report!("failed to deserialize muxr external layout").attach(error))
            .and_then(|layout| layout.validate());

        assert_that!(result, err(anything()));
    }
}
