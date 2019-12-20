use syn::export::{ToTokens, TokenStream2};
//use crate::switch::{SwitchItem, write_for_token, FieldType, unnamed_field_index_item};
use proc_macro2::{Ident, TokenStream};
use syn::{Fields, Field, Type};
use quote::quote;
use crate::switch::SwitchItem;


pub struct FromRoutePart<'a> (pub &'a SwitchItem);

impl <'a> ToTokens for FromRoutePart<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let SwitchItem {
            matcher,
            ident,
            fields,
        } = &self.0;

        let matcher = super::super::build_matcher_from_tokens(&matcher);
        let build_from_captures = build_struct_from_captures(ident, fields);

        tokens.extend(quote! {

            fn from_route_part<__T>(route: String, mut state: Option<__T>) -> (::std::option::Option<Self>, ::std::option::Option<__T>) {

                #matcher
                let route_string = route;

                #build_from_captures

                return (::std::option::Option::None, state)
            }

        })
    }
}

fn build_struct_from_captures(ident: &Ident, fields: &Fields) -> TokenStream2 {
    match fields {
        Fields::Named(named_fields) => {
            let fields: Vec<TokenStream2> = named_fields
                .named
                .iter()
                .filter_map(|field: &Field| {
                    let field_ty: &Type = &field.ty;
                    field.ident.as_ref().map(|i| {
                        let key = i.to_string();
                        (i, key, field_ty)
                    })
                })
                .map(|(field_name, key, field_ty): (&Ident, String, &Type)| {
                    quote! {
                        #field_name: {
                            let (v, s) = match captures.remove(#key) {
                                ::std::option::Option::Some(value) => {
                                    <#field_ty as ::yew_router::Switch>::from_route_part(
                                        value,
                                        state,
                                    )
                                }
                                ::std::option::Option::None => {
                                    (
                                        <#field_ty as ::yew_router::Switch>::key_not_available(),
                                        state,
                                    )
                                }
                            };
                            match v {
                                ::std::option::Option::Some(val) => {
                                    state = s; // Set state for the next var.
                                    val
                                },
                                ::std::option::Option::None => return (::std::option::Option::None, s) // Failed
                            }
                        }
                    }
                })
                .collect();

            return quote! {
                if let ::std::option::Option::Some(mut captures) = matcher.capture_route_into_map(&route_string).ok().map(|x| x.1) {
                    return (
                        ::std::option::Option::Some(
                            #ident {
                                #(#fields),*
                            }
                        ),
                        state
                    );
                };
            };
        }
        Fields::Unnamed(unnamed_fields) => {
            let fields = unnamed_fields.unnamed.iter().map(|f: &Field| {
                let field_ty = &f.ty;
                quote! {
                    {
                        let (v, s) = match drain.next() {
                            ::std::option::Option::Some(value) => {
                                <#field_ty as ::yew_router::Switch>::from_route_part(
                                    value,
                                    state,
                                )
                            },
                            ::std::option::Option::None => {
                                (
                                    <#field_ty as ::yew_router::Switch>::key_not_available(),
                                    state,
                                )
                            }
                        };
                        match v {
                            ::std::option::Option::Some(val) => {
                                state = s; // Set state for the next var.
                                val
                            },
                            ::std::option::Option::None => return (::std::option::Option::None, s) // Failed
                        }
                    }
                }
            });

            quote! {
                if let Some(mut captures) = matcher.capture_route_into_vec(&route_string).ok().map(|x| x.1) {
                    let mut drain = captures.drain(..);
                    return (
                        ::std::option::Option::Some(
                            #ident(
                                #(#fields),*
                            )
                        ),
                        state
                    );
                };
            }
        }
        Fields::Unit => {
            return quote! {
                let mut state = if let ::std::option::Option::Some(_captures) = matcher.capture_route_into_map(&route_string).ok().map(|x| x.1) {
                    return (::std::option::Option::Some(#ident), state);
                } else {
                    state
                };
            }
        }
    }
}
