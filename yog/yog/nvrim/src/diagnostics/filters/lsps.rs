use nvim_oxi::Dictionary;
use ytil_nvim_oxi::dict::DictionaryExt as _;

pub mod harper_ls;
pub mod typos_lsp;

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
