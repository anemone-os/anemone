//! Anemone's kobject's design is a bit different from Linux's, as Rust's type
//! system allows us to do things that are not possible in C. We can use traits
//! and generics to create a more flexible and type-safe kobject system.

use core::{any::Any, fmt::Debug};

use crate::{prelude::*, utils::identity::Identity};

pub type KObjIdent = Identity<{ MAX_FILE_NAME_LEN_BYTES }>;

#[derive(Debug)]
pub struct KObjectBase {
    name: KObjIdent,
    parent: RwLock<Option<Weak<dyn KObject>>>,
}

impl KObjectBase {
    pub fn new(name: KObjIdent) -> Self {
        Self {
            name,
            parent: RwLock::new(None),
        }
    }
}

impl Debug for dyn KObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let base = self.base();
        Debug::fmt(base, f)
    }
}

pub trait KObjectData: Any + Send + Sync {
    fn base(&self) -> &KObjectBase;
}

pub trait KObjectOps {
    // TODO: when sysfs is implemented, add sysfs_ops-like methods here, such as
    // `read_attr`, `write_attr`, etc.
}

impl<T: KObjectData + KObjectOps> KObject for T {}

pub trait KObject: KObjectData + KObjectOps {
    fn name(&self) -> &str {
        self.base().name.as_str()
    }

    fn parent(&self) -> Option<Weak<dyn KObject>> {
        self.base().parent.read_irqsave().clone()
    }

    /// `Arc` is used here since we should ensure that the parent is indeed
    /// alive when we set it as the parent of a child.
    fn set_parent(&mut self, parent: Option<Arc<dyn KObject>>) {
        *self.base().parent.write_irqsave() = parent.as_ref().map(|p| Arc::downgrade(p));
    }
}

#[derive(Debug, KObject)]
pub struct KSet<O: KObject + ?Sized> {
    #[kobject]
    base: KObjectBase,
    children: Vec<Arc<O>>,
}

impl<O: KObject + ?Sized> KSet<O> {
    pub fn new(name: KObjIdent) -> Self {
        Self {
            base: KObjectBase::new(name),
            children: Vec::new(),
        }
    }

    pub fn add_kobject(&mut self, kobject: Arc<O>) {
        self.children.push(kobject);
    }

    pub fn remove_kobject(&mut self, kobject: &Arc<O>) -> Option<Arc<O>> {
        if let Some(pos) = self.children.iter().position(|c| Arc::ptr_eq(c, kobject)) {
            Some(self.children.remove(pos))
        } else {
            None
        }
    }

    /// Get an iterator over the children of this KSet.
    pub fn iter(&self) -> KSetIter<'_, O> {
        KSetIter {
            kset: self,
            index: 0,
        }
    }
}

#[derive(Debug)]
pub struct KSetIter<'a, O: KObject + ?Sized> {
    kset: &'a KSet<O>,
    index: usize,
}

impl<'a, O: KObject + ?Sized> Iterator for KSetIter<'a, O> {
    type Item = &'a Arc<O>;

    fn next(&mut self) -> Option<Self::Item> {
        let children = &self.kset.children;
        if self.index < children.len() {
            let item = &children[self.index];
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }
}
