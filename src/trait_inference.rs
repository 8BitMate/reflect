use crate::{
    CompleteFunction, CompleteImpl, GenericArgument, GenericConstraint, GenericParam, Path,
    PathArguments, PredicateType, Push, TraitBound, Type, TypeEqualitySetRef, TypeNode,
    TypeParamBound,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

/// A set of types that are considered to be equal
pub(crate) struct TypeEqualitySet {
    pub(crate) set: HashSet<Type>,
}

pub(crate) struct ConstraintSet {
    pub(crate) set: HashSet<GenericConstraint>,
}

// A mapping between types and it's corresponding set of equal types
pub(crate) struct TypeEqualitySets {
    set_map: HashMap<Type, TypeEqualitySetRef>,
    sets: Vec<Rc<RefCell<TypeEqualitySet>>>,
}

impl ConstraintSet {
    fn new() -> Self {
        ConstraintSet {
            set: HashSet::new(),
        }
    }

    fn insert(&mut self, constraint: GenericConstraint) -> bool {
        self.set.insert(constraint)
    }

    fn contains(&self, constraint: &GenericConstraint) -> bool {
        self.set.contains(constraint)
    }
}

impl TypeEqualitySet {
    fn new() -> Self {
        TypeEqualitySet {
            set: HashSet::new(),
        }
    }

    fn contains(&self, ty: &Type) -> bool {
        self.set.contains(ty)
    }

    fn insert(&mut self, ty: Type) -> bool {
        self.set.insert(ty)
    }
}

impl TypeEqualitySetRef {
    fn make_most_concrete(
        &self,
        most_concrete_type_map: &mut BTreeMap<Self, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
    ) -> TypeNode {
        use TypeNode::*;
        let most_concrete = most_concrete_type_map.get(self);
        match most_concrete {
            Some(node) => node.clone(),
            None => {
                most_concrete_type_map.insert(*self, Infer);
                let set = &type_equality_sets.sets[self.0].borrow().set;
                let mut iterator = set.iter();
                let first = iterator.next().unwrap().clone().0;
                let most_concrete = iterator.fold(first, |current_most_concrete, ty| {
                    TypeNode::make_most_concrete_from_pair(
                        current_most_concrete,
                        ty.clone().0,
                        most_concrete_type_map,
                        type_equality_sets,
                    )
                });
                most_concrete_type_map.insert(*self, most_concrete.clone());
                most_concrete
            }
        }
    }
}

impl TypeEqualitySets {
    fn new() -> Self {
        TypeEqualitySets {
            set_map: HashMap::new(),
            sets: Vec::new(),
        }
    }

    fn contains_key(&self, ty: &Type) -> bool {
        self.set_map.contains_key(ty)
    }

    fn get_set(&self, ty: &Type) -> Option<Rc<RefCell<TypeEqualitySet>>> {
        self.set_map
            .get(ty)
            .map(|set_ref| self.sets[set_ref.0].clone())
    }

    fn get_set_ref(&self, ty: &Type) -> Option<TypeEqualitySetRef> {
        self.set_map.get(ty).map(|&set_ref| set_ref)
    }

    fn new_set(&mut self, ty: Type) -> TypeEqualitySetRef {
        let mut set = TypeEqualitySet::new();
        set.insert(ty.clone());
        let set = Rc::new(RefCell::new(set));
        let set_ref = self.sets.index_push(set);

        self.set_map.insert(ty, set_ref);
        set_ref
    }

    fn insert_as_equal_to(&mut self, ty1: Type, ty2: Type, constraints: &mut ConstraintSet) {
        use TypeNode::*;
        match (&ty1.0, &ty2.0) {
            (TraitObject(bounds1), TraitObject(bounds2)) => {
                if bounds1.len() != bounds2.len() {
                    panic!("TypeEqualitySets::insert_as_equal_to: TraitObjects have different number of bounds")
                }
                return self.insert_inner_type_as_equal_to(&ty1, &ty2, constraints);
            }
            (TraitObject(bounds), _) => {
                constraints.insert(GenericConstraint::Type(PredicateType {
                    lifetimes: Vec::new(),
                    bounded_ty: ty2,
                    bounds: bounds.clone(),
                }));
                return;
            }
            (_, TraitObject(bounds)) => {
                constraints.insert(GenericConstraint::Type(PredicateType {
                    lifetimes: Vec::new(),
                    bounded_ty: ty1.clone(),
                    bounds: bounds.clone(),
                }));
                return;
            }
            // A reference and a mutable reference are not equal, but a mutable reference may conform to a
            // normal reference, so the inner types may be considered equal
            (Reference { inner: inner1, .. }, ReferenceMut { inner: inner2, .. }) => {
                return self.insert_as_equal_to(
                    Type(*inner1.clone()),
                    Type(*inner2.clone()),
                    constraints,
                )
            }
            (ReferenceMut { inner: inner1, .. }, Reference { inner: inner2, .. }) => {
                return self.insert_as_equal_to(
                    Type(*inner1.clone()),
                    Type(*inner2.clone()),
                    constraints,
                )
            }
            _ => (),
        }
        match self.set_map.get(&ty1) {
            Some(&set_ref) => {
                self.insert_inner_type_as_equal_to(&ty1, &ty2, constraints);
                self.sets[set_ref.0].borrow_mut().insert(ty2.clone());
                self.set_map.insert(ty2, set_ref);
            }
            None => match self.set_map.get(&ty2) {
                Some(&set_ref) => {
                    self.insert_inner_type_as_equal_to(&ty1, &ty2, constraints);
                    self.sets[set_ref.0].borrow_mut().insert(ty1.clone());
                    self.set_map.insert(ty1, set_ref);
                }
                None => {
                    self.insert_inner_type_as_equal_to(&ty1, &ty2, constraints);
                    let mut set = TypeEqualitySet::new();
                    set.insert(ty2.clone());
                    set.insert(ty1.clone());
                    let set = Rc::new(RefCell::new(set));
                    let set_ref = self.sets.index_push(set);
                    self.set_map.insert(ty2, set_ref);
                    self.set_map.insert(ty1, set_ref);
                }
            },
        }
    }

    fn insert_inner_type_as_equal_to(
        &mut self,
        ty1: &Type,
        ty2: &Type,
        constraints: &mut ConstraintSet,
    ) {
        use TypeNode::*;
        match (&ty1.0, &ty2.0) {
            (Tuple(types1), Tuple(types2)) => {
                if types1.len() == types2.len() {
                    types1.iter().zip(types2.iter()).for_each(|(ty1, t2)| {
                        self.insert_as_equal_to(ty1.clone(), t2.clone(), constraints)
                    })
                } else {
                    panic!("TypeEqualitySets::insert_inner_type_as_equal_to: Tuples have different number of arguments")
                }
            }
            (Reference { inner: inner1, .. }, Reference { inner: inner2, .. }) => {
                self.insert_as_equal_to(Type(*inner1.clone()), Type(*inner2.clone()), constraints)
            }
            (ReferenceMut { inner: inner1, .. }, ReferenceMut { inner: inner2, .. }) => {
                self.insert_as_equal_to(Type(*inner1.clone()), Type(*inner2.clone()), constraints)
            }
            (Path(path1), Path(path2)) => {
                self.insert_path_arguments_as_equal_to(path1, path2, constraints);
            }
            (TraitObject(bounds1), TraitObject(bounds2)) => bounds1
                .iter()
                .zip(bounds2.iter())
                .for_each(|bounds| match bounds {
                    (TypeParamBound::Trait(trait_bound1), TypeParamBound::Trait(trait_bound2)) => {
                        self.insert_path_arguments_as_equal_to(
                            &trait_bound1.path,
                            &trait_bound2.path,
                            constraints,
                        );
                    }
                    //FIXME properly deal with lifetimes
                    _ => (),
                }),
            _ => (),
        }
    }

    fn insert_path_arguments_as_equal_to(
        &mut self,
        path1: &Path,
        path2: &Path,
        constraints: &mut ConstraintSet,
    ) {
        let (segment1, segment2) = (
            &path1.path[path1.path.len() - 1],
            &path2.path[path2.path.len() - 1],
        );
        match (&segment1.args, &segment2.args) {
            (PathArguments::AngleBracketed(args1), PathArguments::AngleBracketed(args2))
                if args1.args.args.len() == args2.args.args.len() =>
            {
                args1
                    .args
                    .args
                    .iter()
                    .zip(args2.args.args.iter())
                    .for_each(|args| match args {
                        (GenericArgument::Type(ty1), GenericArgument::Type(ty2)) => {
                            self.insert_as_equal_to(ty1.clone(), ty2.clone(), constraints)
                        }
                        _ => {
                            unimplemented!("TypeEqualitySets::insert_inner_type_as_equal_to: Path")
                        }
                    })
            }
            (PathArguments::Parenthesized(args1), _) => unimplemented!(
                "TypeEqualitySets::insert_inner_type_as_equal_to: ParenthesizedGenericArgument"
            ),
            (_, PathArguments::Parenthesized(args1)) => unimplemented!(
                "TypeEqualitySets::insert_inner_type_as_equal_to: ParenthesizedGenericArgument"
            ),
            _ => (),
        }
    }
}

impl CompleteImpl {
    fn compute_trait_bounds(&self) -> ConstraintSet {
        let mut constraints = ConstraintSet::new();
        let mut type_equality_sets = TypeEqualitySets::new();
        let mut relevant_generic_params = BTreeSet::new();
        let mut most_concrete_type_map = BTreeMap::new();

        if let Type(TypeNode::DataStructure { ref generics, .. }) = self.ty {
            generics.constraints.iter().for_each(|constraint| {
                constraints.insert(constraint.clone());
            });
            generics.params.iter().for_each(|param_ref| {
                relevant_generic_params.insert(*param_ref);
            })
        };

        if let Some(generics) = self
            .trait_ty
            .as_ref()
            .and_then(|trait_ty| trait_ty.generics.as_ref())
        {
            generics.constraints.iter().for_each(|constraint| {
                constraints.insert(constraint.clone());
            });
            generics.params.iter().for_each(|param_ref| {
                relevant_generic_params.insert(*param_ref);
            })
        };

        self.functions.iter().for_each(|function| {
            function.compute_trait_bounds(&mut constraints, &mut type_equality_sets)
        });

        let relevant_generic_params = generic_param_set_refs(
            &relevant_generic_params,
            &mut most_concrete_type_map,
            &mut type_equality_sets,
        );

        let constraints = constraints
            .set
            .into_iter()
            .filter_map(|constraint| {
                constraint.make_most_relevant(
                    &mut most_concrete_type_map,
                    &type_equality_sets,
                    &relevant_generic_params,
                )
            })
            .collect();

        ConstraintSet { set: constraints }
    }
}

/// Find all equality sets of the generic parameters related to the
/// DataStructure or trait that is being implemented, including inner
/// parameters of other types like: the T in some::path<T>
fn generic_param_set_refs(
    relevant_generic_params: &BTreeSet<GenericParam>,
    most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
    type_equality_sets: &mut TypeEqualitySets,
) -> BTreeSet<TypeEqualitySetRef> {
    use TypeNode::*;
    let mut generic_param_set_refs = BTreeSet::new();
    let mut checked_set_refs = BTreeSet::new();

    relevant_generic_params.iter().for_each(|param| {
        let type_param_ref = param.type_param_ref().unwrap();
        let set_ref = type_equality_sets.get_set_ref(&Type(TypeParam(type_param_ref)));
        let set_ref =
            set_ref.unwrap_or_else(|| type_equality_sets.new_set(Type(TypeParam(type_param_ref))));

        if !checked_set_refs.contains(&set_ref) {
            checked_set_refs.insert(set_ref);
            let node = set_ref.make_most_concrete(most_concrete_type_map, type_equality_sets);
            node.inner_param_set_refs(type_equality_sets, &mut generic_param_set_refs)
        }
    });

    generic_param_set_refs
}

impl PredicateType {
    fn is_relevant(
        &self,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> bool {
        self.bounded_ty
            .0
            .is_relevant(&type_equality_sets, &relevant_generic_params)
            && self
                .bounds
                .iter()
                .all(|bound| bound.is_relevant(&type_equality_sets, &relevant_generic_params))
    }

    fn make_most_relevant(
        self,
        most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> Option<Self> {
        match self.bounded_ty.0.make_most_relevant(
            most_concrete_type_map,
            type_equality_sets,
            relevant_generic_params,
        ) {
            Some(bounded_ty) => {
                let bounds = iter_option_to_option_vec(self.bounds.into_iter().map(|bound| {
                    bound.make_most_relevant(
                        most_concrete_type_map,
                        type_equality_sets,
                        relevant_generic_params,
                    )
                }));
                match bounds {
                    Some(bounds) => Some(PredicateType {
                        bounded_ty: Type(bounded_ty),
                        bounds,
                        // FIXME: lifetimes
                        lifetimes: self.lifetimes,
                    }),

                    None => None,
                }
            }

            None => None,
        }
    }
}

/// Transform an iterator of Option values where every item is Some(value)
/// into Some(vec) of those values. If any of the items are None. The returned
/// value is also None
fn iter_option_to_option_vec<T>(
    iterator: impl Iterator<Item = Option<T>> + ExactSizeIterator,
) -> Option<Vec<T>> {
    let vec = Some(Vec::with_capacity(iterator.len()));
    iterator.fold(vec, |vec, val| {
        if let (Some(mut vec), Some(val)) = (vec, val) {
            vec.push(val);
            Some(vec)
        } else {
            None
        }
    })
}

impl TypeNode {
    fn is_relevant(
        &self,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> bool {
        use TypeNode::*;
        match self {
            TypeParam(type_param_ref) => {
                if let Some(set_ref) =
                    type_equality_sets.get_set_ref(&Type(TypeParam(*type_param_ref)))
                {
                    relevant_generic_params.contains(&set_ref)
                } else {
                    false
                }
            }
            Reference { lifetime, inner } => {
                inner.is_relevant(type_equality_sets, relevant_generic_params)
            }
            ReferenceMut { lifetime, inner } => {
                inner.is_relevant(type_equality_sets, relevant_generic_params)
            }
            _ => false,
        }
    }

    fn make_most_relevant(
        self,
        most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> Option<Self> {
        let node = self.make_most_concrete(most_concrete_type_map, type_equality_sets);
        if node.is_relevant(type_equality_sets, relevant_generic_params) {
            Some(node)
        } else {
            None
        }
    }

    fn make_most_concrete(
        self,
        most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
    ) -> Self {
        let ty = Type(self);
        if let Some(set) = type_equality_sets.get_set_ref(&ty) {
            set.make_most_concrete(most_concrete_type_map, type_equality_sets)
        } else {
            ty.0
        }
    }

    fn make_most_concrete_from_pair(
        ty1: TypeNode,
        ty2: TypeNode,
        most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
    ) -> Self {
        use TypeNode::*;
        match (ty1, ty2) {
            (Infer, node) => node.make_most_concrete(most_concrete_type_map, type_equality_sets),
            (node, Infer) => node.make_most_concrete(most_concrete_type_map, type_equality_sets),
            (PrimitiveStr, _) => PrimitiveStr,
            (_, PrimitiveStr) => PrimitiveStr,
            (Path(path1), Path(path2)) => crate::Path::make_most_concrete_from_pair(
                path1,
                path2,
                most_concrete_type_map,
                type_equality_sets,
            ),
            (path @ Path(_), _) => {
                path.make_most_concrete(most_concrete_type_map, type_equality_sets)
            }
            (_, path @ Path(_)) => {
                path.make_most_concrete(most_concrete_type_map, type_equality_sets)
            }
            (Tuple(types1), Tuple(types2)) if types1.len() == types2.len() => Tuple(
                types1
                    .into_iter()
                    .zip(types2.into_iter())
                    .map(|(ty1, ty2)| {
                        Type(TypeNode::make_most_concrete_from_pair(
                            ty1.0,
                            ty2.0,
                            most_concrete_type_map,
                            type_equality_sets,
                        ))
                    })
                    .collect(),
            ),
            (Reference { inner: inner1, .. }, Reference { inner: inner2, .. }) => Reference {
                inner: Box::new(TypeNode::make_most_concrete_from_pair(
                    *inner1,
                    *inner2,
                    most_concrete_type_map,
                    type_equality_sets,
                )),
                // FIXME: deal with lifetimes
                lifetime: None,
            },
            (ReferenceMut { inner: inner1, .. }, ReferenceMut { inner: inner2, .. }) => {
                ReferenceMut {
                    inner: Box::new(TypeNode::make_most_concrete_from_pair(
                        *inner1,
                        *inner2,
                        most_concrete_type_map,
                        type_equality_sets,
                    )),
                    // FIXME: deal with lifetimes
                    lifetime: None,
                }
            }
            (TraitObject(_), node) => {
                node.make_most_concrete(most_concrete_type_map, type_equality_sets)
            }
            (node, TraitObject(_)) => {
                node.make_most_concrete(most_concrete_type_map, type_equality_sets)
            }
            (TypeParam(ref1), TypeParam(ref2)) => {
                if ref1 < ref2 {
                    TypeParam(ref1)
                } else {
                    TypeParam(ref2)
                }
            }
            _ => panic!("TypeNode: make_most_concrete_pair: incompatible types"),
        }
    }

    fn inner_param_set_refs(
        &self,
        type_equality_sets: &mut TypeEqualitySets,
        generic_param_set_refs: &mut BTreeSet<TypeEqualitySetRef>,
    ) {
        use TypeNode::*;
        match self {
            Tuple(types) => {
                for ty in types.iter() {
                    ty.0.inner_param_set_refs(type_equality_sets, generic_param_set_refs)
                }
            }
            Reference { inner, .. } => {
                inner.inner_param_set_refs(type_equality_sets, generic_param_set_refs)
            }
            ReferenceMut { inner, .. } => {
                inner.inner_param_set_refs(type_equality_sets, generic_param_set_refs)
            }
            Path(path) => {
                path.inner_param_set_refs(type_equality_sets, generic_param_set_refs);
            }
            TypeParam(type_param_ref) => {
                let set_ref = type_equality_sets
                    .get_set_ref(&Type(TypeParam(*type_param_ref)))
                    .unwrap_or_else(|| {
                        type_equality_sets.new_set(Type(TypeParam(*type_param_ref)))
                    });
                generic_param_set_refs.insert(set_ref);
            }
            _ => {}
        }
    }
}

impl TypeParamBound {
    fn is_relevant(
        &self,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> bool {
        match self {
            TypeParamBound::Trait(bound) => {
                //FIXME: Properly deal with lifetimes
                bound
                    .path
                    .is_relevant(type_equality_sets, relevant_generic_params)
            }

            TypeParamBound::Lifetime(_) => {
                // FIXME: properly deal with lifetimes
                true
            }
        }
    }

    fn make_most_relevant(
        self,
        most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> Option<Self> {
        match self {
            TypeParamBound::Trait(bound) => {
                if let Some(path) = bound.path.make_most_relevant(
                    most_concrete_type_map,
                    type_equality_sets,
                    relevant_generic_params,
                ) {
                    Some(TypeParamBound::Trait(TraitBound {
                        path,
                        // FIXME: properly deal with lifetimes
                        lifetimes: bound.lifetimes,
                    }))
                } else {
                    None
                }
            }

            // FIXME: properly deal with lifetimes
            bound @ TypeParamBound::Lifetime(_) => Some(bound),
        }
    }
}

impl Path {
    fn is_relevant(
        &self,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> bool {
        self.path.iter().all(|segment| match &segment.args {
            PathArguments::None => true,

            PathArguments::AngleBracketed(args) => args.args.args.iter().all(|arg| match arg {
                GenericArgument::Type(ty) => {
                    ty.0.is_relevant(type_equality_sets, relevant_generic_params)
                }

                GenericArgument::Lifetime(_) => true,

                _ => unimplemented!("is_relevant: PathArguments::AngleBracketed"),
            }),

            PathArguments::Parenthesized(_) => {
                unimplemented!("is_relevant: PathArguments::Parenthesized")
            }
        })
    }

    fn make_most_relevant(
        self,
        most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> Option<Self> {
        todo!()
    }

    fn inner_param_set_refs(
        &self,
        type_equality_sets: &mut TypeEqualitySets,
        generic_param_set_refs: &mut BTreeSet<TypeEqualitySetRef>,
    ) {
        for segment in self.path.iter() {
            match &segment.args {
                PathArguments::None => {}
                PathArguments::AngleBracketed(args) => {
                    for arg in args.args.args.iter() {
                        match arg {
                            GenericArgument::Type(ty) => ty
                                .0
                                .inner_param_set_refs(type_equality_sets, generic_param_set_refs),
                            GenericArgument::Lifetime(lifetime) => {
                                unimplemented!("TypeNode::inner_param_set_refs: Lifetime")
                            }
                            _ => unimplemented!(),
                        }
                    }
                }
                PathArguments::Parenthesized(_) => {
                    unimplemented!("TypeNode::inner_param_set_ref: Parenthesized")
                }
            }
        }
    }

    fn make_most_concrete_from_pair(
        mut path1: Path,
        mut path2: Path,
        most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
    ) -> TypeNode {
        let path1_len = path1.path.len();
        let path2_len = path2.path.len();
        let last_index1 = path1_len - 1;
        let last_index2 = path2_len - 1;
        let segment1 = &mut path1.path[last_index1];
        let segment2 = &mut path2.path[last_index2];
        match (&mut segment1.args, &mut segment2.args) {
            // Assume the path with the fewest arguments is the most concrete
            // since it is likely a type alias of the type with more arguments
            (PathArguments::None, _) => TypeNode::Path(path1),
            (_, PathArguments::None) => TypeNode::Path(path2),
            (PathArguments::AngleBracketed(args1), PathArguments::AngleBracketed(args2)) => {
                let args1 = &mut args1.args.args;
                let args2 = &mut args2.args.args;
                if args1.len() < args2.len() {
                    TypeNode::Path(path1)
                        .make_most_concrete(most_concrete_type_map, type_equality_sets)
                } else if args1.len() > args2.len() {
                    TypeNode::Path(path2)
                        .make_most_concrete(most_concrete_type_map, type_equality_sets)
                } else {
                    // Assume we are dealing with the same type path
                    let args = args1
                        .iter()
                        .zip(args2.iter())
                        .map(|arg_pair| match arg_pair {
                            (GenericArgument::Type(ty1), GenericArgument::Type(ty2)) => {
                                GenericArgument::Type(Type(TypeNode::make_most_concrete_from_pair(
                                    ty1.clone().0,
                                    ty2.clone().0,
                                    most_concrete_type_map,
                                    type_equality_sets,
                                )))
                            }
                            // FIXME: Deal with lifetimes
                            (
                                GenericArgument::Lifetime(lifetime_ref1),
                                GenericArgument::Lifetime(lifetime_ref2),
                            ) => GenericArgument::Lifetime(*lifetime_ref1),
                            _ => unimplemented!(
                                "Path::make_most_concrete_from_pair: GenericArgument"
                            ),
                        })
                        .collect();

                    if path1.global || path1_len < path2_len {
                        *args1 = args;
                        TypeNode::Path(path1)
                    } else {
                        *args2 = args;
                        TypeNode::Path(path2)
                    }
                }
            }
            (PathArguments::Parenthesized(args1), PathArguments::Parenthesized(args2)) => {
                unimplemented!("Path::make_most_concrete_from_pair: Parenthesized")
            }
            _ => panic!("Path::make_most_concrete_from_pair: incompatible types"),
        }
    }
}

impl GenericConstraint {
    fn make_most_relevant(
        self,
        most_concrete_type_map: &mut BTreeMap<TypeEqualitySetRef, TypeNode>,
        type_equality_sets: &TypeEqualitySets,
        relevant_generic_params: &BTreeSet<TypeEqualitySetRef>,
    ) -> Option<Self> {
        match self {
            GenericConstraint::Type(pred_ty) => pred_ty
                .make_most_relevant(
                    most_concrete_type_map,
                    type_equality_sets,
                    relevant_generic_params,
                )
                .map(|pred_ty| GenericConstraint::Type(pred_ty)),

            GenericConstraint::Lifetime(_) =>
            //FIXME: Properly handle lifetimes
            {
                Some(self)
            }
        }
    }
}

impl CompleteFunction {
    fn compute_trait_bounds(
        &self,
        constraints: &mut ConstraintSet,
        type_equality_sets: &mut TypeEqualitySets,
    ) {
        self.invokes.iter().for_each(|invoke| {
            let parent = &invoke.function.parent;
            let sig = &invoke.function.sig;
            sig.inputs
                .iter()
                .zip(invoke.args.iter())
                .for_each(|(ty, val)| {
                    // Make sure inner types are included
                    type_equality_sets.insert_as_equal_to(
                        ty.clone(),
                        val.node().get_type(),
                        constraints,
                    )
                });

            // Add parent constraints
            // FIXME: Add constraints from parent type
            if let Some(generics) = parent.as_ref().and_then(|parent| parent.generics.as_ref()) {
                generics.constraints.iter().for_each(|constraint| {
                    if !constraints.contains(constraint) {
                        constraints.insert(constraint.clone());
                    };
                })
            };

            // Add function constraints
            // FIXME: Add constraints from types in signature
            if let Some(generics) = sig.generics.as_ref() {
                generics.constraints.iter().for_each(|constraint| {
                    if !constraints.contains(constraint) {
                        constraints.insert(constraint.clone());
                    };
                })
            }
        });
    }
}
