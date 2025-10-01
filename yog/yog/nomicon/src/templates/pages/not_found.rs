use askama::Template;

use crate::templates::components::footer::Footer;

#[derive(Template)]
#[template(path = "pages/not_found.html")]
pub struct NotFoundPage {
    pub footer: Footer,
}
