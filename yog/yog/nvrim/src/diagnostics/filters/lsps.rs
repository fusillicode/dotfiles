use nvim_oxi::Dictionary;
use ytil_nvim_oxi::dict::DictionaryExt as _;

pub mod harper_ls;
pub mod typos_lsp;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum GetDiagMsgOutput {
    Msg(String),
    Skip,
}

pub trait LspFilter {
    fn buf_path(&self) -> Option<&str>;

    fn source(&self) -> &str;

    fn get_diag_msg_or_skip(&self, buf_path: &str, lsp_diag: &Dictionary) -> color_eyre::Result<GetDiagMsgOutput> {
        if self.buf_path().is_some_and(|bp| !buf_path.contains(bp)) {
            return Ok(GetDiagMsgOutput::Skip);
        }
        let maybe_diag_source = lsp_diag.get_opt_t::<nvim_oxi::String>("source")?;
        if maybe_diag_source.is_none() || maybe_diag_source.is_some_and(|diag_source| self.source() != diag_source) {
            return Ok(GetDiagMsgOutput::Skip);
        }
        Ok(GetDiagMsgOutput::Msg(lsp_diag.get_t::<nvim_oxi::String>("message")?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_diag_msg_or_skip_when_buf_path_not_matched_returns_skip() {
        let filter = TestFilter {
            source: "Test",
            buf_path: Some("src/"),
        };
        let diag = dict! {
            source: "Test",
            message: "some message",
        };
        assert2::let_assert!(Ok(result) = filter.get_diag_msg_or_skip("tests/main.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Skip);
    }

    #[test]
    fn get_diag_msg_or_skip_when_buf_path_matched_but_source_none_returns_skip() {
        let filter = TestFilter {
            source: "Test",
            buf_path: Some("src/"),
        };
        let diag = dict! {
            message: "some message",
        };
        assert2::let_assert!(Ok(result) = filter.get_diag_msg_or_skip("src/main.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Skip);
    }

    #[test]
    fn get_diag_msg_or_skip_when_buf_path_matched_but_source_mismatch_returns_skip() {
        let filter = TestFilter {
            source: "Test",
            buf_path: Some("src/"),
        };
        let diag = dict! {
            source: "Other",
            message: "some message",
        };
        assert2::let_assert!(Ok(result) = filter.get_diag_msg_or_skip("src/main.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Skip);
    }

    #[test]
    fn get_diag_msg_or_skip_when_buf_path_and_source_matches_returns_msg() {
        let filter = TestFilter {
            source: "Test",
            buf_path: Some("src/"),
        };
        let diag = dict! {
            source: "Test",
            message: "some message",
        };
        assert2::let_assert!(Ok(result) = filter.get_diag_msg_or_skip("src/main.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Msg("some message".to_string()));
    }

    #[test]
    fn get_diag_msg_or_skip_when_no_buf_path_and_source_matches_returns_msg() {
        let filter = TestFilter {
            source: "Test",
            buf_path: None,
        };
        let diag = dict! {
            source: "Test",
            message: "another message",
        };
        assert2::let_assert!(Ok(result) = filter.get_diag_msg_or_skip("any/path.rs", &diag));
        pretty_assertions::assert_eq!(result, GetDiagMsgOutput::Msg("another message".to_string()));
    }

    struct TestFilter {
        source: &'static str,
        buf_path: Option<&'static str>,
    }

    impl LspFilter for TestFilter {
        fn buf_path(&self) -> Option<&str> {
            self.buf_path
        }

        fn source(&self) -> &str {
            self.source
        }
    }
}
