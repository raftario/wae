use darling::FromMeta;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, AttributeArgs, Error, ItemFn, Path};

#[derive(Debug, FromMeta)]
struct EntryArgs {
    #[darling(default)]
    thread_maximum: Option<u32>,
    #[darling(default)]
    thread_minimum: Option<u32>,
    #[darling(default)]
    path: Option<Path>,
}

#[proc_macro_attribute]
pub fn main(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as AttributeArgs);
    let input = parse_macro_input!(input as ItemFn);

    let args = match EntryArgs::from_list(&args) {
        Ok(a) => a,
        Err(err) => {
            return TokenStream::from(err.write_errors());
        }
    };

    match entry(args, input, false) {
        Ok(ts) => ts,
        Err(err) => TokenStream::from(err.to_compile_error()),
    }
}

#[proc_macro_attribute]
pub fn test(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as AttributeArgs);
    let input = parse_macro_input!(input as ItemFn);

    let args = match EntryArgs::from_list(&args) {
        Ok(a) => a,
        Err(err) => {
            return TokenStream::from(err.write_errors());
        }
    };

    match entry(args, input, true) {
        Ok(ts) => ts,
        Err(err) => TokenStream::from(err.to_compile_error()),
    }
}

fn entry(args: EntryArgs, input: ItemFn, test: bool) -> Result<TokenStream, Error> {
    let ItemFn {
        attrs,
        vis,
        mut sig,
        block,
    } = input;
    let EntryArgs {
        thread_maximum,
        thread_minimum,
        path,
    } = args;

    if sig.asyncness.take().is_none() {
        let msg = "the async keyword is missing from the function declaration";
        return Err(Error::new_spanned(sig.fn_token, msg));
    }

    let header = if test {
        quote! { #[::core::prelude::v1::test] }
    } else {
        quote! {}
    };

    let path = path.map(|p| quote! { #p }).unwrap_or(quote! { ::wae });

    let thread_maximum = match thread_maximum {
        Some(maximum) => quote! { .thread_maximum(#maximum) },
        None => quote! {},
    };
    let thread_minimum = match thread_minimum {
        Some(minimum) => quote! { .thread_minimum(#minimum) },
        None => quote! {},
    };

    let output = quote! {
        #header
        #(#attrs)*
        #vis #sig {
            #path::Threadpool::builder()
                #thread_maximum
                #thread_minimum
                .build()
                .unwrap()
                .block_on(async #block)
        }
    };
    Ok(TokenStream::from(output))
}
