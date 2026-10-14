//! Slot storage with stable ids.

use serde::{Deserialize, Serialize};

/// Ids are never reused, so they stay valid in undo snapshots and can be compared across
/// clones (a clone keeps every id).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Arena<T>(Vec<Option<T>>);

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> Arena<T> {
    pub fn insert(&mut self, item: T) -> u32 {
        self.0.push(Some(item));
        (self.0.len() - 1) as u32
    }

    pub fn get(&self, id: u32) -> Option<&T> {
        self.0.get(id as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.0.get_mut(id as usize)?.as_mut()
    }

    pub fn remove(&mut self, id: u32) -> Option<T> {
        self.0.get_mut(id as usize)?.take()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &T)> {
        self.0.iter().enumerate().filter_map(|(i, t)| Some((i as u32, t.as_ref()?)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.0.iter_mut().flatten()
    }

    /// The id the next insert will get.
    pub fn next_id(&self) -> u32 {
        self.0.len() as u32
    }
}
