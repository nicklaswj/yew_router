extern crate proc_macro;
use proc_macro::{TokenStream};
use yew_router_route_parser::{OptimizedToken, CaptureVariant};
use quote::{quote, ToTokens};
use syn::export::TokenStream2;
use proc_macro_hack::proc_macro_hack;
use syn::{Error, Type};

use syn::parse::{Parse, ParseBuffer};
use syn::parse_macro_input;
use syn::Token;
use syn::Expr;
use syn::Ident;
use syn::spanned::Spanned;

enum Either<T, U> {
    Left(T),
    Right(U)
}


/// Parses either:
///
/// `"route_string" => ComponentType` or `"route_string", |matches| {...}` or `"route_string", render_fn``
///
/// The `=>` and `,` are present to distinguish the type capture from the variable ident capture,
/// because they otherwise can't be distinguished.
struct S {
    /// The routing string
    s: String,
    /// The render target, either a Component, whose props will be synthesized from the routing string,
    /// or a function that can render Html
    target: Option<Either<Type, Either<Ident, Expr>>>
}
impl Parse for S {
    fn parse(input: &ParseBuffer) -> Result<Self, Error> {
        let s = input.parse::<syn::LitStr>()?;
        // The specification of a type must be preceded by a "=>"
        let lookahead = input.lookahead1();
        let target: Option<Either<Type, Either<Ident, Expr>>> = if lookahead.peek(Token![=>]) {
            input.parse::<syn::token::FatArrow>()
                .ok()
                .map(|_| {
                    input.parse::<Type>()
                        .map(Either::Left)
                })
                .transpose()?
        } else if lookahead.peek(Token![,]){
            input.parse::<syn::token::Comma>()
                .ok()
                .map(|_| {
                    // attempt to match an ident first
                    input.parse::<syn::Ident>()
                        .map(Either::Left)
                        .or_else(|_| {
                            // Then attempt to get it from an expression
                            input.parse::<syn::Expr>()
                                .and_then(|expr| {
                                    match &expr {
                                        Expr::Closure(_) | Expr::Block(_) | Expr::MethodCall(_) | Expr::Call(_) | Expr::Path(_) => Ok(expr),
                                        Expr::__Nonexhaustive => panic!("nonexhaustive"),
                                        _ => Err(Error::new(expr.span(), "Must be a Component's Type, a Fn(&HashMap<String, String> -> Option<Html<_>>, or expression that can resolve to such a function."))
                                    }
                                })
                                .map(Either::Right)
                        })
                        .map(Either::Right)
                })
                .transpose()?
        } else {
            None
        };

        Ok(
            S {
                s: s.value(),
                target
            }
        )
    }
}

/// Expected to be used like: route!("/route/to/thing" => Component)
#[proc_macro_hack]
pub fn route(input: TokenStream) -> TokenStream {
    let s = parse_macro_input!(input as S);
    let target = s.target;
    let s: String = s.s;

    // Do the parsing at compile time so the user knows if their matcher is malformed.
    // It will still be their responsibility to know that the corresponding Props can be acquired from a path matcher.
    let t = yew_router_route_parser::parse_str_and_optimize_tokens(s.as_str())
        .expect("Invalid Path Matcher")
        .into_iter()
        .map(ShadowOptimizedToken::from);


    let render_fn = match target {
        Some(target) => {
            match target {
                Either::Left(ty) => {
                    quote! {
                        let phantom: std::marker::PhantomData<#ty> = std::marker::PhantomData;
                        let render_fn = Some(yew_router::path_matcher::PathMatcher::<yew_router::Router>::create_render_fn::<#ty>(phantom));
                    }
                }
                Either::Right(ident_or_expr) => {
                    match ident_or_expr {
                        Either::Left(ident) => {
                            quote! {
                                use yew_router::path_matcher::RenderFn as __RenderFn;
                                let f: Box<dyn __RenderFn<_> > = Box::new(#ident);
                                let render_fn = Some(f);
                            }
                        },
                        Either::Right(expr) => {
                            quote! {

                                use yew_router::path_matcher::RenderFn as __RenderFn;
                                let f: Box<dyn __RenderFn<_>> = Box::new(#expr);
                                let render_fn = Some(f);
                            }
                        }
                    }
                }
            }
        },
        None => quote!{
            let render_fn = None;
        }
    };

    let expanded = quote!{
        {
            #render_fn

            yew_router::path_matcher::PathMatcher {
                tokens : vec![#(#t),*],
                render_fn
            }
        }
    };
    TokenStream::from(expanded)
}

impl ToTokens for ShadowOptimizedToken {
    fn to_tokens(&self, ts: &mut TokenStream2) {
        use ShadowOptimizedToken as SOT;
        let t: TokenStream2 = match self {
            SOT::Match(s) => {
                TokenStream2::from(quote!{yew_router::path_matcher::OptimizedToken::Match(#s.to_string())})
            }
            SOT::Capture ( variant ) => {
                TokenStream2::from(quote!{
                    yew_router::path_matcher::OptimizedToken::Capture(#variant)
                })
            }
            SOT::Optional(optional) => {
                TokenStream2::from(quote!{
                    yew_router::path_matcher::OptimizedToken::Optional(vec![#(#optional),*])
                })
            }
        };
        ts.extend(t)
    }
}

/// A shadow of the OptimizedToken type.
/// It should match it exactly so that this macro can expand to the original.
enum ShadowOptimizedToken {
    Match(String),
    Capture(ShadowCaptureVariant),
    Optional(Vec<ShadowOptimizedToken>)
}

enum ShadowCaptureVariant {
    Unnamed, // {} - matches anything
    ManyUnnamed, // {*} - matches over multiple sections
    NumberedUnnamed{sections: usize}, // {4} - matches 4 sections
    Named(String), // {name} - captures a section and adds it to the map with a given name
    ManyNamed(String), // {*:name} - captures over many sections and adds it to the map with a given name.
    NumberedNamed{sections: usize, name: String} // {2:name} - captures a fixed number of sections with a given name.
}

impl ToTokens for ShadowCaptureVariant {

    fn to_tokens(&self, ts: &mut TokenStream2) {
        let t = match self {
            ShadowCaptureVariant::Unnamed => TokenStream2::from(quote!{yew_router::path_matcher::CaptureVariant::Unnamed}),
            ShadowCaptureVariant::ManyUnnamed => TokenStream2::from(quote!{yew_router::path_matcher::CaptureVariant::ManyUnnamed}),
            ShadowCaptureVariant::NumberedUnnamed { sections } => TokenStream2::from(quote!{yew_router::path_matcher::CaptureVariant::NumberedUnnamed{#sections}}),
            ShadowCaptureVariant::Named(name) => TokenStream2::from(quote!{yew_router::path_matcher::CaptureVariant::Named(#name.to_string())}),
            ShadowCaptureVariant::ManyNamed(name) => TokenStream2::from(quote!{yew_router::path_matcher::CaptureVariant::ManyNamed(#name.to_string())}),
            ShadowCaptureVariant::NumberedNamed { sections, name } => TokenStream2::from(quote!{yew_router::path_matcher::CaptureVariant::NumberedNamed{#sections, #name.to_string()}}),
        };
        ts.extend(t)

    }
}

impl From<OptimizedToken> for ShadowOptimizedToken {
    fn from(ot: OptimizedToken) -> Self {
        use OptimizedToken as OT;
        use ShadowOptimizedToken as SOT;
        match ot {
            OT::Match(s) => SOT::Match(s),
            OT::Capture(variant) => SOT::Capture(variant.into()),
            OptimizedToken::Optional(optional) => SOT::Optional(optional.into_iter().map(SOT::from).collect())
        }
    }
}

impl From<CaptureVariant> for ShadowCaptureVariant {

    fn from(cv: CaptureVariant) -> Self {
        use CaptureVariant as CV;
        use ShadowCaptureVariant as SCV;
        match cv {
            CV::Unnamed => SCV::Unnamed,
            CaptureVariant::ManyUnnamed => SCV::ManyUnnamed,
            CaptureVariant::NumberedUnnamed { sections } => SCV::NumberedUnnamed {sections},
            CaptureVariant::Named(name) => SCV::Named(name),
            CaptureVariant::ManyNamed(name) => SCV::ManyNamed(name),
            CaptureVariant::NumberedNamed { sections, name } => SCV::NumberedNamed {sections, name}
        }

    }
}
