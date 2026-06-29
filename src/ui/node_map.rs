use std::borrow::Borrow;
use taffy::prelude::NodeId;

#[derive(Clone, Default, Debug)]
pub struct NodeMap<T> {
    inner: Vec<Option<T>>,
}

impl<T> NodeMap<T> {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { inner: Vec::with_capacity(capacity) }
    }

    #[inline]
    fn get_idx(node: NodeId) -> usize {
        (u64::from(node) & 0xFFFFFFFF) as usize
    }

    #[inline]
    pub fn insert(&mut self, node: NodeId, value: T) -> Option<T> {
        let idx = Self::get_idx(node);
        if idx >= self.inner.len() {
            self.inner.resize_with(idx + 1, || None);
        }
        std::mem::replace(&mut self.inner[idx], Some(value))
    }

    #[inline]
    pub fn get<Q: Borrow<NodeId>>(&self, node: Q) -> Option<&T> {
        let idx = Self::get_idx(*node.borrow());
        self.inner.get(idx).and_then(|opt| opt.as_ref())
    }

    #[inline]
    pub fn get_mut<Q: Borrow<NodeId>>(&mut self, node: Q) -> Option<&mut T> {
        let idx = Self::get_idx(*node.borrow());
        self.inner.get_mut(idx).and_then(|opt| opt.as_mut())
    }

    #[inline]
    pub fn contains_key<Q: Borrow<NodeId>>(&self, node: Q) -> bool {
        self.get(node).is_some()
    }

    #[inline]
    pub fn remove<Q: Borrow<NodeId>>(&mut self, node: Q) -> Option<T> {
        let idx = Self::get_idx(*node.borrow());
        if idx < self.inner.len() {
            self.inner[idx].take()
        } else {
            None
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    #[inline]
    pub fn values(&self) -> NodeMapValues<'_, T> {
        NodeMapValues {
            iter: self.inner.iter(),
        }
    }

    #[inline]
    pub fn iter(&self) -> NodeMapIter<'_, T> {
        NodeMapIter {
            iter: self.inner.iter(),
            idx: 0,
        }
    }
}

pub struct NodeMapIter<'a, T> {
    iter: std::slice::Iter<'a, Option<T>>,
    idx: usize,
}

impl<'a, T> Iterator for NodeMapIter<'a, T> {
    type Item = (NodeId, &'a T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(opt) = self.iter.next() {
            let current_idx = self.idx;
            self.idx += 1;
            if let Some(val) = opt.as_ref() {
                return Some((NodeId::from(current_idx), val));
            }
        }
        None
    }
}

pub struct NodeMapValues<'a, T> {
    iter: std::slice::Iter<'a, Option<T>>,
}

impl<'a, T> Iterator for NodeMapValues<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(opt) = self.iter.next() {
            if let Some(val) = opt.as_ref() {
                return Some(val);
            }
        }
        None
    }
}

impl<'a, T> IntoIterator for &'a NodeMap<T> {
    type Item = (NodeId, &'a T);
    type IntoIter = NodeMapIter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
