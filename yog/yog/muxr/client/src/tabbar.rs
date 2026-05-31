use std::io::Write;

use crossterm::Command;
use crossterm::QueueableCommand;
use crossterm::cursor::MoveTo;
use crossterm::cursor::RestorePosition;
use crossterm::cursor::SavePosition;
use crossterm::style::Attribute;
use crossterm::style::Color;
use crossterm::style::Print;
use crossterm::style::ResetColor;
use crossterm::style::SetAttribute;
use crossterm::style::SetBackgroundColor;
use crossterm::style::SetForegroundColor;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;
use muxr_core::LayoutSnapshot;
use rootcause::prelude::ResultExt;

/// Queue the current tab list into the first terminal row.
///
/// # Errors
/// - The tab-bar commands cannot be written.
pub fn queue(stdout: &mut impl Write, layout: &LayoutSnapshot) -> rootcause::Result<()> {
    queue_command(stdout, SavePosition)?;
    queue_command(stdout, MoveTo(0, 0))?;
    queue_command(stdout, SetBackgroundColor(Color::DarkGrey))?;
    queue_command(stdout, SetForegroundColor(Color::White))?;
    queue_command(stdout, SetAttribute(Attribute::Bold))?;
    queue_command(stdout, Clear(ClearType::CurrentLine))?;
    queue_command(stdout, Print(format_tabbar(layout)))?;
    queue_command(stdout, ResetColor)?;
    queue_command(stdout, SetAttribute(Attribute::Reset))?;
    queue_command(stdout, RestorePosition)?;
    Ok(())
}

fn format_tabbar(layout: &LayoutSnapshot) -> String {
    layout
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            let ordinal = index.saturating_add(1);
            if tab.id == layout.active_tab {
                format!("[{}:{}]", ordinal, tab.title)
            } else {
                format!(" {}:{} ", ordinal, tab.title)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn queue_command<W, C>(stdout: &mut W, command: C) -> rootcause::Result<()>
where
    W: Write,
    C: Command,
{
    Ok(stdout
        .queue(command)
        .map(|_| ())
        .context("failed to write muxr tab bar")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tabbar_when_second_tab_is_active_marks_active_tab() -> rootcause::Result<()> {
        let layout = muxr_core::LayoutSnapshot::new(
            muxr_core::TabId::new("tab-2")?,
            vec![
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new("tab-1")?,
                    "default",
                    muxr_core::PaneId::new("pane-1")?,
                    vec![muxr_core::PaneSnapshot::new(muxr_core::PaneId::new("pane-1")?, "shell")],
                )?,
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new("tab-2")?,
                    "tab 2",
                    muxr_core::PaneId::new("pane-2")?,
                    vec![muxr_core::PaneSnapshot::new(muxr_core::PaneId::new("pane-2")?, "shell")],
                )?,
            ],
        )?;

        pretty_assertions::assert_eq!(format_tabbar(&layout), " 1:default  [2:tab 2]");
        Ok(())
    }

    #[test]
    fn test_queue_when_layout_is_rendered_writes_tabbar_without_flushing() -> rootcause::Result<()> {
        let active_tab = muxr_core::TabId::new("tab-1")?;
        let active_pane = muxr_core::PaneId::new("pane-1")?;
        let pane = muxr_core::PaneSnapshot::new(active_pane.clone(), "shell");
        let tab = muxr_core::TabSnapshot::new(active_tab.clone(), "default", active_pane, vec![pane])?;
        let layout = muxr_core::LayoutSnapshot::new(active_tab, vec![tab])?;
        let mut output = CountingWriter::default();

        queue(&mut output, &layout)?;

        let rendered = output.rendered_string()?;
        assert2::assert!(rendered.contains("[1:default]"));
        pretty_assertions::assert_eq!(output.flushes, 0);
        Ok(())
    }

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl CountingWriter {
        fn rendered_string(&self) -> rootcause::Result<String> {
            Ok(String::from_utf8(self.bytes.clone()).context("muxr tabbar test output was not utf8")?)
        }
    }

    impl std::io::Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes = self.flushes.saturating_add(1);
            Ok(())
        }
    }
}
