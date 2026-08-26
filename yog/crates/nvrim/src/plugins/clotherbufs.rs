use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::BufDeleteOptsBuilder;

pub fn dict() -> Dictionary {
    dict! {
        "close_other_buffers": fn_from!(close_other_buffers),
    }
}

fn close_other_buffers(force_close: Option<bool>) -> Option<()> {
    let cur_buf = nvim_oxi::api::get_current_buf().handle();

    let opts = BufDeleteOptsBuilder::default()
        .force(force_close.is_some_and(std::convert::identity))
        .build();

    for buf in ytil_noxi::mru_buffers::get()? {
        if cur_buf == buf.id || buf.is_term() {
            continue;
        }
        if let Err(err) = Buffer::from(buf.id).delete(&opts) {
            ytil_noxi::notify::error(format!("error closing buffer | buffer={buf:?} error={err:?}"));
        }
    }

    Some(())
}
