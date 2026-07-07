use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::spec::{ApiSpec, AuthoredNames, ModulePath, TypeNameFamily};

#[derive(Debug, Clone, PartialEq)]
pub struct ApiSpecTree<F: TypeNameFamily = AuthoredNames> {
    pub root: ApiSpecNode<F>,
}

impl ApiSpecTree<AuthoredNames> {
    pub fn single(spec: ApiSpec) -> Self {
        Self {
            root: ApiSpecNode::Leaf(ApiSpecLeaf {
                module_path: ModulePath::default(),
                source_root: PathBuf::new(),
                source_path: PathBuf::new(),
                spec,
            }),
        }
    }

    pub fn into_single_spec(self) -> Option<ApiSpec> {
        match self.root {
            ApiSpecNode::Leaf(leaf) => Some(leaf.spec),
            ApiSpecNode::Branch(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApiSpecNode<F: TypeNameFamily = AuthoredNames> {
    Leaf(ApiSpecLeaf<F>),
    Branch(ApiSpecBranch<F>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiSpecBranch<F: TypeNameFamily = AuthoredNames> {
    pub module_path: ModulePath,
    pub children: BTreeMap<String, ApiSpecNode<F>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiSpecLeaf<F: TypeNameFamily = AuthoredNames> {
    pub module_path: ModulePath,
    pub source_root: PathBuf,
    pub source_path: PathBuf,
    pub spec: ApiSpec<F>,
}
