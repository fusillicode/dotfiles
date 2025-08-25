use nvim_oxi::Dictionary;
use nvim_oxi::Function;
use nvim_oxi::Object;

pub fn sort() -> Object {
    Object::from(Function::<Dictionary, nvim_oxi::Result<_>>::from_fn(sort_core))
}

pub fn sort_core(lsp_diags: Dictionary) -> nvim_oxi::Result<Dictionary> {
    // let mut lsp_diags_by_severity = lsp_diags
    //     .as_slice()
    //     .iter()
    //     .enumerate()
    //     .map(|(idx, kv)| {
    //         let dict = Dictionary::try_from(kv.value().clone()).unwrap();
    //         let severity = dict.get("severity").and_then(|so| usize::try_from(so)).unwrap_or(idx);
    //
    //         (severity, dict)
    //     })
    //     .collect::<Vec<_>>();
    //
    // lsp_diags_by_severity.sort_by(|(sev_a, _), (sev_b, _)| sev_a.cmp(sev_b));
    //
    // Ok(lsp_diags_by_severity.into_iter().map(|(_, dict)| dict).collect())

    Ok(lsp_diags)
}
