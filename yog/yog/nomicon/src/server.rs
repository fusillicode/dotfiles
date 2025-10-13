use std::convert::Infallible;

use async_stream::stream;
use axum::Router;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::routing::get;
use datastar::prelude::PatchSignals;

#[shuttle_runtime::main]
async fn main() -> shuttle_axum::ShuttleAxum {
    let router = Router::new().route("/", get(gen_events));

    Ok(router.into())
}

async fn gen_events() -> impl IntoResponse {
    Sse::new(stream! {
        for idx in 0..42  {
            let patch = PatchSignals::new(format!(r#"{{"generating": {idx}}}"#));
            let sse_event = patch.write_as_axum_sse_event();
            yield Ok::<_, Infallible>(sse_event);
        }
    })
}
