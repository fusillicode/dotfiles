use askama::Template;

use crate::templates::components::footer::Footer;

#[derive(Template)]
#[template(path = "pages/index.html")]
pub struct IndexPage<'a> {
    pub crates: &'a [CrateMeta],
    pub footer: Footer,
}

pub struct CrateMeta {
    pub name: String,
    pub description: String,
}
