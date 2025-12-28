use askama::Template;

use crate::templates::components::footer::Footer;
use crate::templates::filters;

#[derive(Template)]
#[template(path = "pages/index.html")]
pub struct IndexPage {
    pub crates: Vec<CrateMeta>,
    pub footer: Footer,
}

pub struct CrateMeta {
    pub name: String,
    pub description: String,
}
