use nvim_oxi::api::opts::EchoOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::serde::Deserializer;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Function;
use nvim_oxi::Object;
use reqwest::blocking::Client;
use serde::Deserialize;

#[nvim_oxi::plugin]
fn ika() -> Dictionary {
    Dictionary::from_iter([("complete", Function::from(complete))])
}

fn complete(Input { params, callback }: Input) {
    // nvim_oxi::api::echo(
    //     [(format!("{params:?}").as_str(), None)],
    //     true,
    //     &EchoOpts::default(),
    // )
    // .unwrap();

    let client = Client::new();

    let data = serde_json::json!({
        "model": "llama3",
        "prompt": "Why is the sky blue?",
        "stream": false,
    });

    let res = client
        .post("http://localhost:11434/api/generate")
        .json(&data)
        .send()
        .unwrap()
        .text()
        .unwrap();

    // nvim_oxi::api::echo(
    //     [(format!("{res:?}").as_str(), None)],
    //     true,
    //     &EchoOpts::default(),
    // )
    // .unwrap();

    let first = Dictionary::from_iter([("label", res)]);
    callback.call(Array::from_iter([first])).unwrap();
}

#[derive(Deserialize, Debug)]
struct Input {
    params: NvimCmpParmas,
    callback: Function<Array, ()>,
}

#[derive(Deserialize, Debug)]
struct NvimCmpParmas {
    context: NvimCmpContext,
}

#[derive(Deserialize, Debug)]
struct NvimCmpContext {
    bufnr: u32,
    filetype: String,
    cursor_line: String,
    cursor_after_line: String,
    cursor_before_line: String,
    aborted: bool,
    cursor: NvimCmpCursor,
}

#[derive(Deserialize, Debug)]
struct NvimCmpCursor {
    character: u32,
    col: u32,
    line: u32,
    row: u32,
}

impl FromObject for Input {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(Deserializer::new(obj)).map_err(Into::into)
    }
}

impl Poppable for Input {
    unsafe fn pop(
        lstate: *mut nvim_oxi::lua::ffi::lua_State,
    ) -> Result<Self, nvim_oxi::lua::Error> {
        let obj = Object::pop(lstate)?;
        Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
    }
}
