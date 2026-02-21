//! Proc-macro companion for `ytil_sys`.
//!
//! If [`try_trait_v2`](https://github.com/rust-lang/rust/issues/84277) stabilises, this crate
//! could be replaced by a `CliResult` newtype implementing `Termination` + `FromResidual`.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

/// Wraps `fn main() -> rootcause::Result<()>` into `ytil_sys::run` so errors print in bold red.
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as syn::ItemFn);
    let body = &input_fn.block;
    let attrs: Vec<_> = input_fn.attrs.iter().collect();

    let output = quote! {
        #(#attrs)*
        fn main() {
            ytil_sys::run(|| #body);
        }
    };

    output.into()
}
