use crate::{
    generics, Data, GenericConstraint, GenericParam, Generics, Ident, LifetimeRef, ParamMap, Path,
    Print, TypeParamBound, TypeParamRef, TYPE_PARAMS,
};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use ref_cast::RefCast;
use std::collections::BTreeMap;
use std::fmt::Debug;
use syn::TypePath;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Type(pub(crate) TypeNode);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TypeNode {
    Infer,
    Tuple(Vec<Type>),
    PrimitiveStr,
    Reference {
        lifetime: Option<LifetimeRef>,
        inner: Box<TypeNode>,
    },
    ReferenceMut {
        lifetime: Option<LifetimeRef>,
        inner: Box<TypeNode>,
    },
    Dereference(Box<TypeNode>),
    TraitObject(Vec<TypeParamBound>),
    DataStructure {
        name: Ident,
        generics: Generics,
        data: Data<Type>,
    },
    Path(Path),
    TypeParam(TypeParamRef),
}

impl Type {
    pub fn unit() -> Self {
        Type(TypeNode::Tuple(Vec::new()))
    }

    pub fn tuple(types: &[Self]) -> Self {
        Type(TypeNode::Tuple(Vec::from(types)))
    }

    pub fn primitive_str() -> Self {
        Type(TypeNode::PrimitiveStr)
    }

    pub fn reference(&self) -> Self {
        Type(TypeNode::Reference {
            lifetime: None,
            inner: Box::new(self.0.clone()),
        })
    }

    pub fn reference_mut(&self) -> Self {
        Type(TypeNode::ReferenceMut {
            lifetime: None,
            inner: Box::new(self.0.clone()),
        })
    }

    pub fn dereference(&self) -> Self {
        match &self.0 {
            TypeNode::Reference { inner, .. } => Type((**inner).clone()),
            TypeNode::ReferenceMut { inner, .. } => Type((**inner).clone()),
            other => Type(TypeNode::Dereference(Box::new(other.clone()))),
        }
    }

    pub fn data(&self) -> Data<Self> {
        match &self.0 {
            TypeNode::DataStructure { data, .. } => data.clone().map(|field| field.element),
            TypeNode::Reference { lifetime, inner } => {
                Type((**inner).clone()).data().map(|field| {
                    Type(TypeNode::Reference {
                        lifetime: lifetime.clone(),
                        inner: Box::new(field.element.0),
                    })
                })
            }
            TypeNode::ReferenceMut { lifetime, inner } => {
                Type((**inner).clone()).data().map(|field| {
                    Type(TypeNode::ReferenceMut {
                        lifetime: lifetime.clone(),
                        inner: Box::new(field.element.0),
                    })
                })
            }
            _ => panic!("Type::data"),
        }
    }

    /// Returns a Type from a Tuple
    pub fn get_tuple_type(&self, index: usize) -> Self {
        match &self.0 {
            TypeNode::Tuple(types) => types[index].clone(),
            _ => panic!("Type::get_tuple_type: Not a Tuple"),
        }
    }

    pub fn get_trait_object(type_param_bounds: &[&str], param_map: &mut ParamMap) -> Self {
        Type(TypeNode::TraitObject(
            type_param_bounds
                .iter()
                .map(|bound| TypeParamBound::get_type_param_bound(bound, param_map))
                .collect(),
        ))
    }

    /// Get a trait object without generics
    pub fn get_simple_trait_object(type_param_bounds: &[&str]) -> Self {
        Type(TypeNode::TraitObject(
            type_param_bounds
                .iter()
                .map(|bound| TypeParamBound::get_simple_type_param_bound(bound))
                .collect(),
        ))
    }

    pub(crate) fn syn_to_type(ty: syn::Type, param_map: &mut ParamMap) -> Self {
        match ty {
            syn::Type::Path(TypePath {
                //FIXME: add qself to Path
                qself: None,
                path,
            }) => {
                if let Some(ident) = path.get_ident() {
                    if let Some(&param) = param_map.get(ident) {
                        return Type(TypeNode::TypeParam(
                            param
                                .type_param_ref()
                                .expect("syn_to_type: Not a type param ref"),
                        ));
                    }
                }
                Type(TypeNode::Path(Path::syn_to_path(path, param_map)))
            }

            syn::Type::Reference(reference) => {
                let inner = Box::new(Type::syn_to_type(*reference.elem, param_map).0);
                let lifetime = reference.lifetime.map(|lifetime| {
                    param_map
                        .get(&lifetime.ident)
                        .and_then(|&param| GenericParam::lifetime_ref(param))
                        .expect("syn_to_type: Not a lifetime ref")
                });
                if reference.mutability.is_some() {
                    Type(TypeNode::ReferenceMut { lifetime, inner })
                } else {
                    Type(TypeNode::Reference { lifetime, inner })
                }
            }

            syn::Type::TraitObject(type_trait_object) => Type(TypeNode::TraitObject(
                generics::syn_to_type_param_bounds(type_trait_object.bounds, param_map).collect(),
            )),

            syn::Type::Tuple(type_tuple) => {
                if type_tuple.elems.is_empty() {
                    Type::unit()
                } else if type_tuple.elems.len() == 1 && !type_tuple.elems.trailing_punct() {
                    // It is not a tuple. The parentheses were just used to
                    // disambiguate the type.
                    Self::syn_to_type(type_tuple.elems.into_iter().next().unwrap(), param_map)
                } else {
                    Type(TypeNode::Tuple(
                        type_tuple
                            .elems
                            .into_iter()
                            .map(|elem| Self::syn_to_type(elem, param_map))
                            .collect(),
                    ))
                }
            }
            _ => unimplemented!("Type::syn_to_type"),
        }
    }

    pub(crate) fn name_and_generics(
        &self,
    ) -> (TokenStream, Vec<GenericParam>, Vec<GenericConstraint>) {
        self.0.name_and_generics()
    }

    pub(crate) fn clone_with_fresh_generics(
        &self,
        ref_map: &BTreeMap<GenericParam, GenericParam>,
    ) -> Self {
        Type(self.0.clone_with_fresh_generics(ref_map))
    }
}

impl TypeNode {
    pub(crate) fn get_name(&self) -> String {
        match self {
            //FIXME: Add more TypeNode branches
            TypeNode::Tuple(types) => {
                let types = types.iter().map(Print::ref_cast);
                quote!((#(#types),*)).to_string()
            }
            TypeNode::PrimitiveStr => String::from("str"),
            TypeNode::DataStructure { name, .. } => name.to_string(),
            TypeNode::Reference { inner, .. } => (&**inner).get_name(),
            TypeNode::ReferenceMut { inner, .. } => (&**inner).get_name(),
            TypeNode::Path(path) => {
                let mut tokens = TokenStream::new();
                Print::ref_cast(path).to_tokens(&mut tokens);
                tokens.to_string()
            }
            TypeNode::TypeParam(type_param_ref) => {
                TYPE_PARAMS.with(|params| params.borrow()[type_param_ref.0].ident.to_string())
            }

            _ => panic!("Type::get_name"),
        }
    }

    pub(crate) fn name_and_generics(
        &self,
    ) -> (TokenStream, Vec<GenericParam>, Vec<GenericConstraint>) {
        use super::TypeNode::*;
        match self {
            Infer => panic!("Type::name_and_generics: Infer"),

            Tuple(types) => {
                let types = types.iter().map(Print::ref_cast);
                (quote!((#(#types),*)), Vec::new(), Vec::new())
            }

            PrimitiveStr => (quote!(str), Vec::new(), Vec::new()),

            Reference { lifetime, inner } => {
                let lifetime = lifetime.as_ref().map(Print::ref_cast);
                let (name, params, constraints) = inner.name_and_generics();
                (quote!(& #lifetime #name), params, constraints)
            }

            ReferenceMut { lifetime, inner } => {
                let lifetime = lifetime.as_ref().map(Print::ref_cast);
                let (name, params, constraints) = inner.name_and_generics();
                (quote!(&mut #lifetime #name), params, constraints)
            }

            Dereference(_dereference) => panic!("Type::name_and_generics: Dereference"),

            TraitObject(type_param_bound) => {
                if type_param_bound.len() != 1 {
                    panic!("Type::name_and_generics: TraitObject has more than one bound")
                }
                let type_param_bound = Print::ref_cast(&type_param_bound[0]);
                (quote!(dyn #type_param_bound), Vec::new(), Vec::new())
            }

            DataStructure {
                name,
                generics:
                    Generics {
                        params,
                        constraints,
                    },
                ..
            } => (quote!(#name), params.clone(), constraints.clone()),

            Path(path) => {
                //FIXME: separate generics from path if possible
                let path = Print::ref_cast(path);
                (quote!(path), Vec::new(), Vec::new())
            }

            TypeParam(type_param_ref) => (
                quote!(),
                vec![GenericParam::Type(*type_param_ref)],
                Vec::new(),
            ),
        }
    }

    pub(crate) fn clone_with_fresh_generics(
        &self,
        ref_map: &BTreeMap<GenericParam, GenericParam>,
    ) -> Self {
        use super::TypeNode::*;
        match self {
            Infer => Infer,

            Tuple(types) => Tuple(
                types
                    .iter()
                    .map(|ty| ty.clone_with_fresh_generics(ref_map))
                    .collect(),
            ),

            PrimitiveStr => PrimitiveStr,

            Reference { lifetime, inner } => Reference {
                lifetime: lifetime.map(|lifetime_ref| {
                    ref_map
                        .get(&GenericParam::Lifetime(lifetime_ref))
                        .and_then(|param| param.lifetime_ref())
                        .unwrap()
                }),
                inner: Box::new(inner.clone_with_fresh_generics(ref_map)),
            },

            ReferenceMut { lifetime, inner } => ReferenceMut {
                lifetime: lifetime.map(|lifetime_ref| {
                    ref_map
                        .get(&GenericParam::Lifetime(lifetime_ref))
                        .and_then(|param| param.lifetime_ref())
                        .unwrap()
                }),
                inner: Box::new(inner.clone_with_fresh_generics(ref_map)),
            },

            Dereference(dereference) => {
                Dereference(Box::new(dereference.clone_with_fresh_generics(ref_map)))
            }

            TraitObject(bounds) => TraitObject(
                bounds
                    .iter()
                    .map(|bound| bound.clone_with_fresh_generics(ref_map))
                    .collect(),
            ),

            DataStructure { .. } => {
                unimplemented!("Type::clone_with_fresh_generics: DataStructure")
            }

            Path(path) => Path(path.clone_with_fresh_generics(ref_map)),

            TypeParam(type_param_ref) => TypeParam(
                ref_map
                    .get(&GenericParam::Type(*type_param_ref))
                    .and_then(|param| param.type_param_ref())
                    .unwrap(),
            ),
        }
    }
}
