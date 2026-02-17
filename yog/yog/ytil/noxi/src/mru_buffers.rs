//! Most recently used (MRU) buffers parsing from Nvim's buffer list.

use std::str::FromStr;

use nvim_oxi::api::Buffer;
use rootcause::prelude::ResultExt as _;
use rootcause::report;

/// Represents a most recently used buffer with its metadata.
#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct MruBuffer {
    /// The buffer ID.
    pub id: i32,
    /// Whether the buffer is unlisted.
    pub is_unlisted: bool,
    /// The buffer name.
    pub name: String,
    /// The kind of buffer based on its name.
    pub kind: BufferKind,
}

impl MruBuffer {
    pub const fn is_term(&self) -> bool {
        match self.kind {
            BufferKind::Term => true,
            BufferKind::GrugFar | BufferKind::Path | BufferKind::NoName => false,
        }
    }
}

impl From<&MruBuffer> for Buffer {
    fn from(value: &MruBuffer) -> Self {
        Self::from(value.id)
    }
}

/// Categorizes buffers by their type based on name patterns.
#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum BufferKind {
    /// Terminal buffers starting with "term://".
    Term,
    /// Grug FAR results buffers.
    GrugFar,
    /// Regular file path buffers.
    Path,
    /// No name buffers.
    NoName,
}

impl<T: AsRef<str>> From<T> for BufferKind {
    fn from(value: T) -> Self {
        let str = value.as_ref();
        if str.starts_with("term://") {
            Self::Term
        } else if str.starts_with("Grug FAR") {
            Self::GrugFar
        } else if str.starts_with("[No Name]") {
            Self::NoName
        } else {
            Self::Path
        }
    }
}

/// Parses a line from Nvim's buffer list output into an [`MruBuffer`].
///
/// # Errors
/// - Parsing the buffer ID fails.
/// - Extracting the unlisted flag fails.
/// - Extracting the name fails.
impl FromStr for MruBuffer {
    type Err = rootcause::Report;

    fn from_str(mru_buffer_line: &str) -> Result<Self, Self::Err> {
        let mru_buffer_line = mru_buffer_line.trim();

        let is_unlisted_idx = mru_buffer_line
            .char_indices()
            .find_map(|(idx, c)| if c.is_numeric() { None } else { Some(idx) })
            .ok_or_else(|| report!("error finding buffer id end"))
            .attach_with(|| format!("mru_buffer_line={mru_buffer_line:?}"))?;

        let id: i32 = {
            let id = mru_buffer_line
                .get(..is_unlisted_idx)
                .ok_or_else(|| report!("error extracting buffer id"))
                .attach_with(|| format!("mru_buffer_line={mru_buffer_line:?}"))?;
            id.parse()
                .context("error parsing buffer id")
                .attach_with(|| format!("id={id:?} mru_buffer_line={mru_buffer_line:?}"))?
        };

        let is_unlisted = mru_buffer_line
            .get(is_unlisted_idx..=is_unlisted_idx)
            .ok_or_else(|| report!("error extracting is_unlisted by idx"))
            .attach_with(|| format!("idx={is_unlisted_idx} mru_buffer_line={mru_buffer_line:?}"))?
            == "u";

        // Find the opening '"' after the flags and extract the name between the quotes.
        // Nvim's `:ls` format is `%3d%c%c%c%c%c "%s"` (5 flag chars + space + quoted name),
        // but we locate the quote dynamically to be resilient to format changes.
        let name_idx = mru_buffer_line
            .get(is_unlisted_idx..)
            .and_then(|s| s.find('"').map(|i| is_unlisted_idx.saturating_add(i).saturating_add(1)))
            .ok_or_else(|| report!("error finding opening quote"))
            .attach_with(|| format!("mru_buffer_line={mru_buffer_line:?}"))?;

        let rest = mru_buffer_line
            .get(name_idx..)
            .ok_or_else(|| report!("error extracting name part by idx"))
            .attach_with(|| format!("idx={name_idx} mru_buffer_line={mru_buffer_line:?}"))?;

        let (name, _) = rest
            .split_once('"')
            .ok_or_else(|| report!("error extracting name"))
            .attach_with(|| format!("rest={rest:?} mru_buffer_line={mru_buffer_line:?}"))?;

        Ok(Self {
            id,
            is_unlisted,
            name: name.to_string(),
            kind: BufferKind::from(name),
        })
    }
}

/// Retrieves the list of most recently used buffers from Nvim.
///
/// Calls Nvim's "execute" function with "ls t" to get the buffer list output,
/// then parses it into a vector of [`MruBuffer`]. Errors during execution or parsing
/// are notified to the user and result in [`None`] being returned.
pub fn get() -> Option<Vec<MruBuffer>> {
    let Ok(mru_buffers_output) = nvim_oxi::api::call_function::<_, String>("execute", ("ls t",))
        .inspect_err(|err| crate::notify::error(format!("error getting mru buffers | error={err:?}")))
    else {
        return None;
    };

    parse_mru_buffers_output(&mru_buffers_output)
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error parsing mru buffers output | mru_buffers_output={mru_buffers_output:?} error={err:?}"
            ));
        })
        .ok()
}

/// Parses the output of Nvim's "ls t" command into a vector of [`MruBuffer`].
///
/// # Errors
/// - Parsing any individual buffer line fails.
fn parse_mru_buffers_output(mru_buffers_output: &str) -> rootcause::Result<Vec<MruBuffer>> {
    if mru_buffers_output.is_empty() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for mru_buffer_line in mru_buffers_output.lines() {
        if mru_buffer_line.is_empty() {
            continue;
        }
        out.push(MruBuffer::from_str(mru_buffer_line)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Test data matches Nvim's real `:ls` format: `%3d%c%c%c%c%c "%s"`
    // i.e. 5 flag chars (unlisted, current/alt, active/hidden, ro, changed) + space + quoted name.
    #[rstest]
    #[case(
        "1u%a   \"file.txt\"",
        MruBuffer {
            id: 1,
            is_unlisted: true,
            name: "file.txt".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "2  %a  \"another.txt\"",
        MruBuffer {
            id: 2,
            is_unlisted: false,
            name: "another.txt".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "3  %a  \"[No Name]\"",
        MruBuffer {
            id: 3,
            is_unlisted: false,
            name: "[No Name]".to_string(),
            kind: BufferKind::NoName,
        }
    )]
    #[case(
        "4u  a  \"term://bash\"",
        MruBuffer {
            id: 4,
            is_unlisted: true,
            name: "term://bash".to_string(),
            kind: BufferKind::Term,
        }
    )]
    #[case(
        "5  %a  \"Grug FAR results\"",
        MruBuffer {
            id: 5,
            is_unlisted: false,
            name: "Grug FAR results".to_string(),
            kind: BufferKind::GrugFar,
        }
    )]
    #[case(
        "  6  %a  \"trimmed.txt\"  ",
        MruBuffer {
            id: 6,
            is_unlisted: false,
            name: "trimmed.txt".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "10 #h   \"multi_digit.txt\"",
        MruBuffer {
            id: 10,
            is_unlisted: false,
            name: "multi_digit.txt".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "7u  aR  \"term://~//12345:/bin/zsh\"",
        MruBuffer {
            id: 7,
            is_unlisted: true,
            name: "term://~//12345:/bin/zsh".to_string(),
            kind: BufferKind::Term,
        }
    )]
    fn from_str_when_valid_input_returns_mru_buffer(#[case] input: &str, #[case] expected: MruBuffer) {
        let result = MruBuffer::from_str(input);
        assert2::assert!(let Ok(mru_buffer) = result);
        pretty_assertions::assert_eq!(mru_buffer, expected);
    }

    #[rstest]
    #[case("", "error finding buffer id end")]
    #[case(" %a  \"file.txt\"", "error parsing buffer id")]
    #[case("au %a  \"file.txt\"", "error parsing buffer id")]
    #[case("1u%a  \"file.txt", "error extracting name")]
    #[case("1u%a  file.txt", "error finding opening quote")]
    fn from_str_when_invalid_input_returns_error(#[case] input: &str, #[case] expected_err_substr: &str) {
        let result = MruBuffer::from_str(input);
        assert2::assert!(let Err(err) = result);
        assert!(err.to_string().contains(expected_err_substr));
    }
}
