use askama::Template;

use crate::templates::components::footer::Footer;
use crate::templates::filters;

#[derive(Template)]
#[template(path = "pages/not_found.html")]
pub struct NotFoundPage {
    pub footer: Footer,
}
