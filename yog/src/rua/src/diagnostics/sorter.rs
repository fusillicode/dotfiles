use nvim_oxi::Dictionary;
use nvim_oxi::Integer;

pub fn sort(mut lsp_diags: Vec<Dictionary>) -> Vec<Dictionary> {
    lsp_diags.sort_by_key(get_severity_or_default);
    lsp_diags
}

fn get_severity_or_default(dict: &Dictionary) -> Integer {
    dict.get("severity")
        .map(|o| Integer::try_from(o.clone()))
        .unwrap_or(Ok(Integer::MIN))
        .unwrap_or(Integer::MIN)
}
