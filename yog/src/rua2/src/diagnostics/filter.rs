use nvim_oxi::Dictionary;
use nvim_oxi::Function;
use nvim_oxi::Object;

pub fn filter() -> Object {
    Object::from(Function::<Vec<Dictionary>, _>::from_fn(filter_core))
}

pub fn filter_core(lsp_diags: Vec<Dictionary>) -> Vec<Dictionary> {
    lsp_diags
}
