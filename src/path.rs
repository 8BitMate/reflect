use crate::{GenericArgument, GenericArguments, GenericParam, Ident, ParamMap, Type};
use std::collections::BTreeMap;
use syn::parse::{Parse, ParseStream, Result};
use syn::{parse_str, ReturnType, Token};

#[derive(Debug, Clone)]
pub struct Path {
    pub(crate) global: bool,
    pub(crate) path: Vec<PathSegment>,
}

pub(crate) struct SimplePath {
    pub(crate) path: Path,
}

#[derive(Debug, Clone)]
pub(crate) struct PathSegment {
    pub(crate) ident: Ident,
    pub(crate) args: PathArguments,
}

#[derive(Debug, Clone)]
pub(crate) enum PathArguments {
    None,
    AngleBracketed(AngleBracketedGenericArguments),
    Parenthesized(ParenthesizedGenericArguments),
}

#[derive(Debug, Clone)]
pub(crate) struct AngleBracketedGenericArguments {
    pub(crate) args: GenericArguments,
}

/// Arguments of a function path segment: the `(A, B) -> C` in `Fn(A, B) -> C`.
#[derive(Debug, Clone)]
pub(crate) struct ParenthesizedGenericArguments {
    /// (A, B)
    pub(crate) inputs: Vec<Type>,
    /// C
    pub(crate) output: Option<Type>,
}

impl Path {
    pub(crate) fn root() -> Self {
        Path {
            global: true,
            path: Vec::new(),
        }
    }

    pub(crate) fn empty() -> Self {
        Path {
            global: false,
            path: Vec::new(),
        }
    }

    /// Get a simple path without generics
    pub(crate) fn get_simple_path(&self, ident: &str) -> Self {
        let mut path = self.clone();
        path.path.push(PathSegment {
            ident: Ident::from(
                parse_str::<syn::Ident>(ident).expect("Path::get_simple_path: Not an Ident"),
            ),
            args: PathArguments::None,
        });
        path
    }

    pub fn path_from_str(path: &str, param_map: &mut ParamMap) -> Self {
        Self::syn_to_path(
            parse_str(path).expect("Path::path_from_str: Not a Path"),
            param_map,
        )
    }

    pub fn simple_path_from_str(path: &str) -> Self {
        parse_str::<SimplePath>(path)
            .expect("simple_path_from_str: Not a simple path")
            .path
    }

    pub(crate) fn syn_to_path(path: syn::Path, param_map: &mut ParamMap) -> Self {
        let global = path.leading_colon.is_some();
        let path: Vec<_> = path
            .segments
            .into_iter()
            .map(|segment| Self::syn_to_path_segment(segment, param_map))
            .collect();
        Path { global, path }
    }

    pub(crate) fn syn_to_path_segment(
        path_segment: syn::PathSegment,
        param_map: &mut ParamMap,
    ) -> PathSegment {
        let syn::PathSegment { ident, arguments } = path_segment;
        let ident = Ident::from(ident);

        match arguments {
            syn::PathArguments::None => PathSegment {
                ident,
                args: PathArguments::None,
            },
            syn::PathArguments::AngleBracketed(generic_args) => PathSegment {
                ident,
                args: PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                    args: GenericArguments {
                        args: generic_args
                            .args
                            .into_iter()
                            .map(|arg| GenericArgument::syn_to_generic_argument(arg, param_map))
                            .collect(),
                    },
                }),
            },

            syn::PathArguments::Parenthesized(parenthesized) => PathSegment {
                ident,
                args: PathArguments::Parenthesized(ParenthesizedGenericArguments {
                    inputs: parenthesized
                        .inputs
                        .into_iter()
                        .map(|input| Type::syn_to_type(input, param_map))
                        .collect(),
                    output: match parenthesized.output {
                        ReturnType::Default => None,
                        ReturnType::Type(_, ty) => Some(Type::syn_to_type(*ty, param_map)),
                    },
                }),
            },
        }
    }

    pub(crate) fn ident_to_path(ident: Ident) -> Path {
        Path {
            global: false,
            path: vec![PathSegment {
                ident,
                args: PathArguments::None,
            }],
        }
    }

    pub(crate) fn clone_with_fresh_generics(
        &self,
        ref_map: &BTreeMap<GenericParam, GenericParam>,
    ) -> Self {
        Path {
            global: self.global,
            path: self
                .path
                .iter()
                .map(|segment| match segment.args {
                    PathArguments::None => PathSegment {
                        ident: segment.ident.clone(),
                        args: PathArguments::None,
                    },

                    PathArguments::AngleBracketed(ref args) => PathSegment {
                        ident: segment.ident.clone(),
                        args: PathArguments::AngleBracketed(
                            args.clone_with_fresh_generics(ref_map),
                        ),
                    },

                    PathArguments::Parenthesized(ref args) => PathSegment {
                        ident: segment.ident.clone(),
                        args: PathArguments::Parenthesized(args.clone_with_fresh_generics(ref_map)),
                    },
                })
                .collect(),
        }
    }
}

impl AngleBracketedGenericArguments {
    pub(crate) fn clone_with_fresh_generics(
        &self,
        ref_map: &BTreeMap<GenericParam, GenericParam>,
    ) -> Self {
        AngleBracketedGenericArguments {
            args: self.args.clone_with_fresh_generics(ref_map),
        }
    }
}

impl ParenthesizedGenericArguments {
    pub(crate) fn clone_with_fresh_generics(
        &self,
        ref_map: &BTreeMap<GenericParam, GenericParam>,
    ) -> Self {
        ParenthesizedGenericArguments {
            inputs: self
                .inputs
                .iter()
                .map(|ty| ty.clone_with_fresh_generics(ref_map))
                .collect(),
            output: self
                .output
                .as_ref()
                .map(|ty| ty.clone_with_fresh_generics(ref_map)),
        }
    }
}

impl Parse for SimplePath {
    fn parse(input: ParseStream) -> Result<Self> {
        let global = input.parse::<Option<Token![::]>>()?.is_some();
        let first = Ident::from(input.parse::<syn::Ident>()?);
        let mut path = vec![PathSegment {
            ident: first,
            args: PathArguments::None,
        }];
        while !input.is_empty() {
            input.parse::<Token![::]>()?;
            path.push(PathSegment {
                ident: Ident::from(input.parse::<syn::Ident>()?),
                args: PathArguments::None,
            });
        }
        Ok(SimplePath {
            path: Path { global, path },
        })
    }
}
