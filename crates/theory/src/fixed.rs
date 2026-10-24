//! A fixed-capacity vector that needs no heap.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::ops::{Deref, DerefMut};

/// Returned when a [`FixedVec`] is already full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapacityError;

impl fmt::Display for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("capacity exceeded")
    }
}

/// A vector with a compile-time maximum length, stored inline.
#[derive(Clone, Copy)]
pub struct FixedVec<T: Copy + Default, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> FixedVec<T, N> {
    /// An empty vector.
    pub fn new() -> Self {
        Self {
            items: [T::default(); N],
            len: 0,
        }
    }

    /// Copies `items` into a new vector, or fails if there are more than `N`.
    pub fn from_slice(items: &[T]) -> Result<Self, CapacityError> {
        let mut v = Self::new();
        for &item in items {
            v.push(item)?;
        }
        Ok(v)
    }

    /// The maximum number of items.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Whether no more items fit.
    pub fn is_full(&self) -> bool {
        self.len == N
    }

    /// Appends an item.
    pub fn push(&mut self, item: T) -> Result<(), CapacityError> {
        if self.len == N {
            return Err(CapacityError);
        }
        self.items[self.len] = item;
        self.len += 1;
        Ok(())
    }

    /// Removes and returns the last item.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.items[self.len])
    }

    /// Inserts an item at `index`, shifting later items right.
    ///
    /// # Panics
    /// If `index > len`.
    pub fn insert(&mut self, index: usize, item: T) -> Result<(), CapacityError> {
        assert!(index <= self.len, "insert index out of bounds");
        if self.len == N {
            return Err(CapacityError);
        }
        self.items.copy_within(index..self.len, index + 1);
        self.items[index] = item;
        self.len += 1;
        Ok(())
    }

    /// Removes and returns the item at `index`, shifting later items left.
    ///
    /// # Panics
    /// If `index >= len`.
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "remove index out of bounds");
        let item = self.items[index];
        self.items.copy_within(index + 1..self.len, index);
        self.len -= 1;
        item
    }

    /// Keeps only the items for which `keep` returns true.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut write = 0;
        for read in 0..self.len {
            let item = self.items[read];
            if keep(&item) {
                self.items[write] = item;
                write += 1;
            }
        }
        self.len = write;
    }

    /// Shortens the vector to `len` items.
    pub fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    /// Removes all items.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// The items as a slice.
    pub fn as_slice(&self) -> &[T] {
        &self.items[..self.len]
    }

    /// The items as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items[..self.len]
    }
}

impl<T: Copy + Default, const N: usize> Default for FixedVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize> Deref for FixedVec<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: Copy + Default, const N: usize> DerefMut for FixedVec<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: Copy + Default + PartialEq, const N: usize> PartialEq for FixedVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + Default + Eq, const N: usize> Eq for FixedVec<T, N> {}

impl<T: Copy + Default + Hash, const N: usize> Hash for FixedVec<T, N> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<T: Copy + Default + fmt::Debug, const N: usize> fmt::Debug for FixedVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl<'a, T: Copy + Default, const N: usize> IntoIterator for &'a FixedVec<T, N> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_insert_remove() {
        let mut v: FixedVec<u8, 4> = FixedVec::new();
        v.push(1).unwrap();
        v.push(3).unwrap();
        v.insert(1, 2).unwrap();
        assert_eq!(v.as_slice(), &[1, 2, 3]);
        assert_eq!(v.remove(0), 1);
        assert_eq!(v.as_slice(), &[2, 3]);
        v.push(4).unwrap();
        v.push(5).unwrap();
        assert_eq!(v.push(6), Err(CapacityError));
        v.retain(|&x| x % 2 == 0);
        assert_eq!(v.as_slice(), &[2, 4]);
    }

    #[test]
    fn equality_ignores_unused_slots() {
        let mut a: FixedVec<u8, 4> = FixedVec::from_slice(&[1, 2, 9]).unwrap();
        a.pop();
        let b: FixedVec<u8, 4> = FixedVec::from_slice(&[1, 2]).unwrap();
        assert_eq!(a, b);
    }
}
