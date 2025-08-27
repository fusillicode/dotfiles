use nvim_oxi::Dictionary;
use nvim_oxi::Function;
use nvim_oxi::Integer;
use nvim_oxi::Object;

pub fn sort() -> Object {
    Object::from(Function::<Vec<Dictionary>, _>::from_fn(sort_core))
}

fn sort_core(mut lsp_diags: Vec<Dictionary>) -> Vec<Dictionary> {
    lsp_diags.sort_by_key(get_severity_or_default);
    lsp_diags
}

fn get_severity_or_default(dict: &Dictionary) -> Integer {
    dict.get("severity")
        .map(|o| Integer::try_from(o.clone()))
        .unwrap_or(Ok(Integer::MIN))
        .unwrap_or(Integer::MIN)
}
