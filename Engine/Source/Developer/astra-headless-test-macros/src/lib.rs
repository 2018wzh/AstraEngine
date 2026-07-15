use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

#[proc_macro_attribute]
pub fn test(_args: TokenStream, item: TokenStream) -> TokenStream {
    expand(item, false)
}

#[proc_macro_attribute]
pub fn tokio_test(_args: TokenStream, item: TokenStream) -> TokenStream {
    expand(item, true)
}

fn expand(item: TokenStream, asynchronous: bool) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let attrs = function.attrs;
    let vis = function.vis;
    let sig = function.sig;
    let body = function.block;
    if asynchronous {
        quote! { #(#attrs)* #[::tokio::test] #vis #sig { let _astra_headless_context = ::astra_headless_test::HeadlessTestContext::start_async().await.expect("headless test session must start"); #body } }.into()
    } else {
        quote! { #(#attrs)* #[test] #vis #sig { let _astra_headless_context = ::astra_headless_test::HeadlessTestContext::start().expect("headless test session must start"); #body } }.into()
    }
}
