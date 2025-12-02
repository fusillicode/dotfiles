use std::str::FromStr;

use color_eyre::eyre::Context as _;
use color_eyre::eyre::eyre;
use nvim_oxi::api::Buffer;

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct MruBuffer {
    pub id: i32,
    pub is_unlisted: bool,
    pub name: String,
    pub kind: BufferKind,
}

impl From<&MruBuffer> for Buffer {
    fn from(value: &MruBuffer) -> Self {
        Self::from(value.id)
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum BufferKind {
    Term,
    GrugFar,
    Path,
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

impl FromStr for MruBuffer {
    type Err = color_eyre::eyre::Error;

    fn from_str(mru_buffer_line: &str) -> Result<Self, Self::Err> {
        let mru_buffer_line = mru_buffer_line.trim();

        let is_unlisted_idx = mru_buffer_line
            .char_indices()
            .find_map(|(idx, c)| if c.is_numeric() { None } else { Some(idx) })
            .ok_or_else(|| eyre!("error finding buffer id end | mru_buffer_line={mru_buffer_line:?}"))?;

        let id: i32 = {
            let id = mru_buffer_line
                .get(..is_unlisted_idx)
                .ok_or_else(|| eyre!("error extracting buffer id | mru_buffer_line={mru_buffer_line:?}"))?;
            id.parse()
                .wrap_err_with(|| format!("error parsing buffer id | id={id:?} mru_buffer_line={mru_buffer_line:?}"))?
        };

        let is_unlisted = mru_buffer_line.get(is_unlisted_idx..=is_unlisted_idx).ok_or_else(|| {
            eyre!("error extracting is_unlisted by idx | idx={is_unlisted_idx} mru_buffer_line={mru_buffer_line:?}")
        })? == "u";

        // Skip entirely the other flags and the first '"' char.
        let name_idx = is_unlisted_idx.saturating_add(7);

        let rest = mru_buffer_line.get(name_idx..).ok_or_else(|| {
            eyre!("error extracting name part by idx | idx={name_idx} mru_buffer_line={mru_buffer_line:?}")
        })?;

        let (name, _) = rest
            .split_once('"')
            .ok_or_else(|| eyre!("error extracting name | rest={rest:?} mru_buffer_line={mru_buffer_line:?}"))?;

        Ok(Self {
            id,
            is_unlisted,
            name: name.to_string(),
            kind: BufferKind::from(name),
        })
    }
}

pub fn get() -> Option<Vec<MruBuffer>> {
    let Ok(mru_buffers_output) = nvim_oxi::api::call_function::<_, String>("execute", ("ls t",))
        .inspect_err(|err| crate::notify::error(format!("error getting mru buffers | err={err:?}")))
    else {
        return None;
    };

    parse_mru_buffers_output(&mru_buffers_output)
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error parsing mru buffers output | mru_buffers_output={mru_buffers_output:?} err={err:?}"
            ));
        })
        .ok()
}

fn parse_mru_buffers_output(mru_buffers_output: &str) -> color_eyre::Result<Vec<MruBuffer>> {
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

    #[rstest]
    #[case(
        "1u %a \"file.txt\"",
        MruBuffer {
            id: 1,
            is_unlisted: true,
            name: "ile.txt".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "2  %a \"another.txt\"",
        MruBuffer {
            id: 2,
            is_unlisted: false,
            name: "nother.txt".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "3  %a \"[No Name]\"",
        MruBuffer {
            id: 3,
            is_unlisted: false,
            name: "No Name]".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "4  %a \"term://bash\"",
        MruBuffer {
            id: 4,
            is_unlisted: false,
            name: "erm://bash".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "5  %a \"Grug FAR results\"",
        MruBuffer {
            id: 5,
            is_unlisted: false,
            name: "rug FAR results".to_string(),
            kind: BufferKind::Path,
        }
    )]
    #[case(
        "  6  %a \"trimmed.txt\"  ",
        MruBuffer {
            id: 6,
            is_unlisted: false,
            name: "rimmed.txt".to_string(),
            kind: BufferKind::Path,
        }
    )]
    fn from_str_when_valid_input_returns_mru_buffer(#[case] input: &str, #[case] expected: MruBuffer) {
        let result = MruBuffer::from_str(input);
        assert2::let_assert!(Ok(mru_buffer) = result);
        pretty_assertions::assert_eq!(mru_buffer, expected);
    }

    #[rstest]
    #[case("", "error finding buffer id end")]
    #[case(" %a \"file.txt\"", "error parsing buffer id")]
    #[case("au %a \"file.txt\"", "error parsing buffer id")]
    #[case("1u %a \"file.txt", "error extracting name")]
    #[case("1u %a file.txt", "error extracting name")]
    fn from_str_when_invalid_input_returns_error(#[case] input: &str, #[case] expected_err_substr: &str) {
        let result = MruBuffer::from_str(input);
        assert2::let_assert!(Err(err) = result);
        assert!(err.to_string().contains(expected_err_substr));
    }
}
