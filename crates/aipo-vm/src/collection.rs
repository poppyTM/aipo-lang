//! Insertion-ordered storage with a revision for structural changes.
use std::ops::{Deref, Index, IndexMut};

/// A vector whose shape changes can be detected even when its length is restored.
/// Element replacement does not change its structural revision.
#[derive(Debug, Clone)]
pub struct Collection<T> {
    items: Vec<T>,
    revision: u64,
}
impl<T: PartialEq> PartialEq for Collection<T> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}
impl<T: PartialEq> PartialEq<Vec<T>> for Collection<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        &self.items == other
    }
}
impl<T: PartialEq> PartialEq<[T]> for Collection<T> {
    fn eq(&self, other: &[T]) -> bool {
        self.items == other
    }
}
impl<T: PartialEq, const N: usize> PartialEq<[T; N]> for Collection<T> {
    fn eq(&self, other: &[T; N]) -> bool {
        self.items == other
    }
}
impl<T: PartialEq, const N: usize> PartialEq<&[T; N]> for Collection<T> {
    fn eq(&self, other: &&[T; N]) -> bool {
        self.items == other.as_slice()
    }
}
impl<T: Eq> Eq for Collection<T> {}
impl<T> Default for Collection<T> {
    fn default() -> Self {
        Vec::new().into()
    }
}
impl<T> From<Vec<T>> for Collection<T> {
    fn from(items: Vec<T>) -> Self {
        Self { items, revision: 0 }
    }
}
impl<T> From<Collection<T>> for Vec<T> {
    fn from(items: Collection<T>) -> Self {
        items.items
    }
}
impl<T> FromIterator<T> for Collection<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        iter.into_iter().collect::<Vec<_>>().into()
    }
}
impl<T> IntoIterator for Collection<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
impl<'a, T> IntoIterator for &'a Collection<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}
impl<T> Deref for Collection<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.items
    }
}
impl<T, I: std::slice::SliceIndex<[T]>> Index<I> for Collection<T> {
    type Output = I::Output;
    fn index(&self, index: I) -> &Self::Output {
        &self.items[index]
    }
}
impl<T, I: std::slice::SliceIndex<[T]>> IndexMut<I> for Collection<T> {
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.items[index]
    }
}
impl<T> Collection<T> {
    /// Current structural revision, independent of length and element values.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    /// Appends an element.
    pub fn push(&mut self, value: T) {
        self.changed();
        self.items.push(value);
    }
    /// Removes the last element.
    pub fn pop(&mut self) -> Option<T> {
        let out = self.items.pop();
        if out.is_some() {
            self.changed();
        }
        out
    }
    /// Inserts an element, shifting following elements.
    pub fn insert(&mut self, index: usize, value: T) {
        self.items.insert(index, value);
        self.changed();
    }
    /// Removes an element, shifting following elements.
    pub fn remove(&mut self, index: usize) -> T {
        let out = self.items.remove(index);
        self.changed();
        out
    }
    /// Removes all elements.
    pub fn clear(&mut self) {
        if !self.items.is_empty() {
            self.changed();
            self.items.clear();
        }
    }
    /// Appends all elements from an iterator.
    pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let old = self.items.len();
        self.items.extend(iter);
        if old != self.items.len() {
            self.changed();
        }
    }
    /// Replaces elements without changing the structure.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.items.iter_mut()
    }
    /// Mutable contiguous elements; cannot resize the collection.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items
    }
    /// Changes ordering.
    pub fn reverse(&mut self) {
        if self.items.len() > 1 {
            self.changed();
            self.items.reverse();
        }
    }
    /// Sorts elements in place.
    pub fn sort_by<F: FnMut(&T, &T) -> std::cmp::Ordering>(&mut self, compare: F) {
        if self.items.len() > 1 {
            self.changed();
            self.items.sort_by(compare);
        }
    }
}
