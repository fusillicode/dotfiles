use rootcause::report;
use serde::Deserialize;

/// External JSON layout used only to seed a brand-new muxr session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalSessionLayout {
    tabs: Vec<ExternalLayoutTab>,
}

impl ExternalSessionLayout {
    /// Validate the layout seed before creating runtime panes.
    ///
    /// # Errors
    /// Returns an error when the layout has no tabs or contains blank cwd or command fields.
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

/// One tab in an external muxr layout seed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalLayoutTab {
    cwd: String,
    panes: Vec<ExternalLayoutPane>,
}

impl ExternalLayoutTab {
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn panes(&self) -> &[ExternalLayoutPane] {
        &self.panes
    }

    fn validate(&self, tab_index: usize) -> rootcause::Result<()> {
        if self.cwd.trim().is_empty() {
            return Err(report!("muxr external layout tab cwd is empty").attach(format!("tab_index={tab_index}")));
        }
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
    cmd: Option<String>,
}

impl ExternalLayoutPane {
    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn cmd(&self) -> Option<&str> {
        self.cmd.as_deref()
    }

    fn validate(&self, tab_index: usize, pane_index: usize) -> rootcause::Result<()> {
        if self.cmd.as_ref().is_some_and(|cmd| cmd.trim().is_empty()) {
            return Err(report!("muxr external layout pane cmd is empty")
                .attach(format!("tab_index={tab_index}"))
                .attach(format!("pane_index={pane_index}")));
        }
        if self.cmd.is_none() && !self.args.is_empty() {
            return Err(report!("muxr external layout pane command args require cmd")
                .attach(format!("tab_index={tab_index}"))
                .attach(format!("pane_index={pane_index}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        pretty_assertions::assert_eq!(tabs.len(), 2);
        pretty_assertions::assert_eq!(tabs[0].cwd(), "/tmp/one");
        pretty_assertions::assert_eq!(tabs[1].cwd(), "/tmp/two");

        let demo_process = tabs[1]
            .panes()
            .first()
            .ok_or_else(|| report!("expected demo process command pane"))?;
        pretty_assertions::assert_eq!(demo_process.cmd(), Some("demo"));
        pretty_assertions::assert_eq!(demo_process.args(), ["process", "start"]);
        Ok(())
    }

    #[rstest::rstest]
    #[case::no_tabs(r#"{"tabs":[]}"#)]
    #[case::empty_cwd(r#"{"tabs":[{"cwd":"","panes":[{}]}]}"#)]
    #[case::no_panes(r#"{"tabs":[{"cwd":"/tmp","panes":[]}]}"#)]
    #[case::empty_cmd(r#"{"tabs":[{"cwd":"/tmp","panes":[{"cmd":"","args":[]}]}]}"#)]
    #[case::args_without_cmd(r#"{"tabs":[{"cwd":"/tmp","panes":[{"args":["server","start"]}]}]}"#)]
    fn test_external_session_layout_validate_when_required_fields_are_empty_returns_error(#[case] raw: &str) {
        let layout: ExternalSessionLayout = serde_json::from_str(raw).expect("test layout json must parse");

        assert2::assert!(layout.validate().is_err());
    }
}
