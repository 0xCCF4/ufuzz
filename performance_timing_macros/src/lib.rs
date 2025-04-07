extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, ToTokens};

fn track_time_impl(attr: TokenStream, item: TokenStream, exclusive: bool) -> TokenStream {
    let input = syn::parse::<syn::Item>(item.clone());
    let block = syn::parse::<syn::Expr>(item.clone());
    let impl_block = syn::parse::<syn::ItemImpl>(item.clone());

    let random_number = rand::random::<u64>() % 1_000_000;
    let timing_measurement = format_ident!("___timing_measurement_{:03}", random_number);

    let mut annotation_name = if attr.is_empty() {
        None
    } else {
        Some(syn::parse_macro_input!(attr as syn::LitStr).value())
    };

    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap();

    let method = if exclusive {
        quote::quote! { begin }
    } else {
        quote::quote! { begin }
    };

    if let Ok(syn::ItemImpl {
        attrs,
        defaultness,
        unsafety,
        impl_token,
        generics,
        trait_,
        self_ty,
        brace_token: _,
        items,
    }) = impl_block
    {
        let items_annotated = items.iter().map(|item| {
            if let syn::ImplItem::Fn(syn::ImplItemFn {
                attrs,
                vis,
                defaultness,
                sig,
                block,
            }) = item
            {
                let annotation_name = annotation_name
                    .clone()
                    .map(|v| {
                        let func_name = &sig.ident;
                        format!("{}::{func_name}", v)
                    })
                    .unwrap_or_else(|| {
                        let func_name = &sig.ident;
                        let type_name = self_ty.to_token_stream().to_string();
                        let trait_name = trait_
                            .as_ref()
                            .map(|(_, path, _)| {
                                path.segments.last().unwrap().to_token_stream().to_string()
                            })
                            .map(|x| format!("::{x}"))
                            .unwrap_or_default();
                        format!("{crate_name}::{type_name}{trait_name}::{func_name}")
                    });
                quote::quote! {
                    #(#attrs)*
                    #[track_time(#annotation_name)]
                    #vis #defaultness #sig #block
                }
            } else {
                quote::quote! { #item }
            }
        });

        let trait_ = if let Some((mark, path, for_t)) = &trait_ {
            quote::quote! { #mark #path #for_t }
        } else {
            quote::quote! {}
        };

        return quote::quote! {
            #(#attrs)*
            #defaultness #unsafety #impl_token #generics #trait_ #self_ty {
                #(#items_annotated)*
            }
        }
        .into();
    } else if let Ok(input) = input {
        if let syn::Item::Fn(ref func) = input {
            let func_name = &func.sig.ident;

            if annotation_name.is_none() {
                annotation_name = Some(format!("{crate_name}::{func_name}_{random_number:03}"));
            }

            let syn::ItemFn {
                sig,
                attrs,
                block,
                vis,
            } = func;

            let annotation_name = annotation_name.unwrap();

            return quote::quote! {
            #(#attrs)*
            #vis #sig {
                let #timing_measurement = ::performance_timing::TimeMeasurement::#method(#annotation_name);
                let r = {
                    #block
                };
                #[allow(unreachable_code)]
                {
                    drop(#timing_measurement);
                    r
                }
            }
        }
                .into();
        }
    } else if let Ok(ref block) = block {
        if annotation_name.is_none() {
            return quote::quote! {
                compile_error!("#[track_time...] requires a name");
            }
            .into();
        }
        let annotation_name = annotation_name.unwrap();

        match block {
            syn::Expr::Block(ref block) => {
                return quote::quote! {
                    {
                        let #timing_measurement = ::performance_timing::TimeMeasurement::#method(#annotation_name);
                        #block
                        #[allow(unreachable_code)]
                        {
                            drop(#timing_measurement);
                        }
                    }
                }
                .into();
            }
            syn::Expr::Loop(syn::ExprLoop { attrs, label, loop_token, body }) => {
                return quote::quote! {
                    #(#attrs)* #label #loop_token {
                        let #timing_measurement = ::performance_timing::TimeMeasurement::#method(#annotation_name);
                        #body
                        #[allow(unreachable_code)]
                        {
                            drop(#timing_measurement);
                        }
                    }
                }
                .into();
            }
            syn::Expr::ForLoop(syn::ExprForLoop { attrs, label, for_token, pat, in_token, expr, body }) => {
                return quote::quote! {
                    #(#attrs)* #label #for_token #pat #in_token #expr {
                        let #timing_measurement = ::performance_timing::TimeMeasurement::#method(#annotation_name);
                        #body
                        #[allow(unreachable_code)]
                        {
                            drop(#timing_measurement);
                        }
                    }
                }
                .into();
            }
            syn::Expr::Call(syn::ExprCall {attrs, func, args, paren_token:_}) => {
                return quote::quote! {
                    #(#attrs)* {
                        let #timing_measurement = ::performance_timing::TimeMeasurement::#method(#annotation_name);
                        let result = #func(#args);
                        #[allow(unreachable_code)]
                        {
                            drop(#timing_measurement);
                            result
                        }
                    }
                }.into()
            }
            _ => {}
        }
    }

    quote::quote! {
        compile_error!("#[track_time] can not be applied to this item");
    }
    .into()
}

#[proc_macro_attribute]
pub fn track_time(attr: TokenStream, item: TokenStream) -> TokenStream {
    track_time_impl(attr, item, true)
}
