use core::ops::RangeBounds;
use std::collections::vec_deque::Drain;
use std::collections::VecDeque;
use std::fmt::Display;
use std::hash::Hash;
use std::ops::Bound;
use std::ops::Index;
use std::ops::IndexMut;

use serde::Deserialize;
use serde::Serialize;

use crate::Lockable;

pub trait RingIndex: Copy + PartialEq + Eq + Hash + PartialOrd + Ord + Display {
    const DEFAULT: Self;

    fn from_usize(index: usize) -> Self;

    fn into_usize(self) -> usize;

    #[inline]
    fn previous(self) -> Self {
        Self::from_usize(self.into_usize().saturating_sub(1))
    }

    #[inline]
    fn next(self) -> Self {
        Self::from_usize(self.into_usize() + 1)
    }

    #[inline]
    fn even(self) -> bool {
        self.into_usize() % 2 == 0
    }
}

pub trait RingElement {
    type Index: RingIndex;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Ring<T: RingElement> {
    elements: VecDeque<T>,
    focused: usize,
}

impl<T: RingElement> Default for Ring<T> {
    fn default() -> Self {
        Self {
            elements: VecDeque::default(),
            focused: 0,
        }
    }
}

impl<T: RingElement> Ring<T> {
    /// Sets the focused index to `idx`.
    pub fn focus(&mut self, idx: T::Index) {
        self.focused = idx.into_usize();
    }

    /// Returns the current focused index.
    pub fn focused_idx(&self) -> T::Index {
        T::Index::from_usize(self.focused)
    }

    /// Returns a reference to the currently focused element, or `None` if out of bounds.
    pub fn focused(&self) -> Option<&T> {
        self.elements.get(self.focused)
    }

    /// Returns a mutable reference to the currently focused element, or `None` if out of bounds.
    pub fn focused_mut(&mut self) -> Option<&mut T> {
        self.elements.get_mut(self.focused)
    }

    /// Returns the number of elements in the ring.
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Returns `true` if the ring contains no elements.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Returns the last index of elements in the ring if any
    pub fn last_index(&self) -> T::Index {
        T::Index::from_usize(self.elements.len().saturating_sub(1))
    }

    /// Returns the last index of elements in the ring if any
    pub fn next_index(&self) -> T::Index {
        T::Index::from_usize(self.elements.len())
    }

    /// Returns an iterator over references to the elements.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.elements.iter()
    }

    /// Returns an iterator over mutable references to the elements.
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> {
        self.elements.iter_mut()
    }

    /// Returns an iterator over references to the elements with indexes.
    pub fn indexed(&self) -> impl DoubleEndedIterator<Item = (T::Index, &T)> {
        self.elements
            .iter()
            .enumerate()
            .map(|(idx, elm)| (T::Index::from_usize(idx), elm))
    }

    /// Returns an iterator over mutable references to the elements with indexes.
    pub fn indexed_mut(&mut self) -> impl DoubleEndedIterator<Item = (T::Index, &mut T)> {
        self.elements
            .iter_mut()
            .enumerate()
            .map(|(idx, elm)| (T::Index::from_usize(idx), elm))
    }

    /// Consumes `Ring<T>`, transforms each element into `U`, and collects them into a new ring.
    /// The focused index is preserved as-is.
    pub fn map_clone<U: RingElement, F>(&self, mut f: F) -> Ring<U>
    where
        F: FnMut(&T) -> U,
    {
        Ring {
            elements: self.elements.iter().map(&mut f).collect(),
            focused: self.focused,
        }
    }

    /// Returns a reference to the element at `index`, or `None` if out of bounds.
    pub fn get(&self, index: T::Index) -> Option<&T> {
        self.elements.get(index.into_usize())
    }

    /// Returns a mutable reference to the element at `index`, or `None` if out of bounds.
    pub fn get_mut(&mut self, index: T::Index) -> Option<&mut T> {
        self.elements.get_mut(index.into_usize())
    }

    /// Tests if any element of the ring matches a predicate.
    pub fn any(&self, f: impl FnMut(&T) -> bool) -> bool {
        self.elements.iter().any(f)
    }

    /// Returns `true` if the ring contains the specified element.
    pub fn contains(&self, x: &T) -> bool
    where
        T: PartialEq<T>,
    {
        self.elements.contains(x)
    }

    /// Searches for an element in the ring, returning its index.
    #[inline]
    pub fn position(&self, predicate: impl FnMut(&T) -> bool) -> Option<T::Index> {
        self.elements
            .iter()
            .position(predicate)
            .map(T::Index::from_usize)
    }

    /// Returns a reference to the first element, or `None` if empty.
    pub fn front(&self) -> Option<&T> {
        self.elements.front()
    }

    /// Returns a mutable reference to the first element, or `None` if empty.
    pub fn front_mut(&mut self) -> Option<&mut T> {
        self.elements.front_mut()
    }

    /// Returns a reference to the last element, or `None` if empty.
    pub fn back(&self) -> Option<&T> {
        self.elements.back()
    }

    /// Returns a mutable reference to the last element, or `None` if empty.
    pub fn back_mut(&mut self) -> Option<&mut T> {
        self.elements.back_mut()
    }

    /// Inserts an element at the front of the ring.
    pub fn push_front(&mut self, value: T) {
        self.elements.push_front(value);
    }

    /// Inserts an element at the back of the ring.
    pub fn push_back(&mut self, value: T) {
        self.elements.push_back(value);
    }

    /// Inserts an element at the specified index, shifting later elements.
    pub fn insert(&mut self, index: T::Index, value: T) {
        self.elements.insert(index.into_usize(), value);
    }

    /// Swaps the elements at indices `i` and `j`.
    pub fn swap(&mut self, i: T::Index, j: T::Index) {
        self.elements.swap(i.into_usize(), j.into_usize());
    }

    /// Changes the length of the ring, either truncating or extending with clones of `value`.
    pub fn resize(&mut self, new_len: usize, value: T)
    where
        T: Clone,
    {
        self.elements.resize(new_len, value);
    }

    /// Retains only the elements specified by the predicate.
    pub fn retain(&mut self, f: impl FnMut(&T) -> bool) {
        self.elements.retain(f);
    }

    /// Removes and returns the element at `index`, or `None` if out of bounds.
    pub fn remove(&mut self, index: T::Index) -> Option<T> {
        self.elements.remove(index.into_usize())
    }

    /// Removes and returns the first element, or `None` if empty.
    pub fn pop_front(&mut self) -> Option<T> {
        self.elements.pop_front()
    }

    /// Removes and returns the last element, or `None` if empty.
    pub fn pop_back(&mut self) -> Option<T> {
        self.elements.pop_back()
    }

    /// Creates a draining iterator over the specified range of elements.
    pub fn drain(&mut self, range: impl RangeBounds<T::Index>) -> Drain<'_, T> {
        let start_bound = match range.start_bound() {
            Bound::Included(&i) => Bound::Included(i.into_usize()),
            Bound::Excluded(&i) => Bound::Excluded(i.into_usize()),
            Bound::Unbounded => Bound::Unbounded,
        };
        let end_bound = match range.end_bound() {
            Bound::Included(&i) => Bound::Included(i.into_usize()),
            Bound::Excluded(&i) => Bound::Excluded(i.into_usize()),
            Bound::Unbounded => Bound::Unbounded,
        };

        self.elements.drain((start_bound, end_bound))
    }

    /// Makes the contents contiguous, returning a mutable slice.
    pub fn make_contiguous(&mut self) -> &mut [T] {
        self.elements.make_contiguous()
    }

    #[cfg(test)]
    pub fn to_vec<U>(&self, f: impl FnMut(&T) -> U) -> Vec<U> {
        self.iter().map(f).collect()
    }
}

impl<T: RingElement + Lockable> Ring<T> {
    /// Insert `value` at logical index `idx`, with trying to keep locked elements
    /// (`is_locked()`) anchored at their original positions.
    ///
    /// Returns the final index of the inserted element.
    pub fn insert_respecting_locks(&mut self, idx: T::Index, value: T) -> T::Index {
        // 1. Bounds check: if index is out of range, simply append.
        let mut idx = idx.into_usize();
        if idx >= self.elements.len() {
            self.elements.push_back(value);
            return T::Index::from_usize(self.len() - 1); // last index
        }

        // 2. Normal VecDeque insertion
        self.elements.insert(idx, value);

        // 3. Walk left-to-right once, swapping any misplaced locked element. After
        // the VecDeque::insert all items after `idx` have moved right by one. For every locked
        // element that is now to the right of an unlocked one, swap it back left exactly once.
        for index in (idx + 1)..self.elements.len() {
            if self.elements[index].locked() && !self.elements[index - 1].locked() {
                self.elements.swap(index - 1, index);

                // If the element we just inserted participated in the swap,
                // update `idx` so we can return its final location.
                if idx == index - 1 {
                    idx = index;
                }
            }
        }
        T::Index::from_usize(idx)
    }

    /// Remove element at `idx`, with trying to keep locked elements
    /// (`is_locked()`) anchored at their original positions.
    ///
    /// Returns the removed element, or `None` if `idx` is out of bounds.
    pub fn remove_respecting_locks(&mut self, idx: T::Index) -> Option<T> {
        // 1. Bounds check: if index is out of range, do nothing.
        let idx = idx.into_usize();
        if idx >= self.elements.len() {
            return None;
        }

        // 2. Remove the element at the requested index.
        //    All elements after idx are now shifted left by 1.
        let removed = self.elements.remove(idx)?;

        // 3. If less than 2 elements remain, nothing to shift.
        if self.elements.len() < 2 {
            return Some(removed);
        }

        // 4. Iterate from the element just after the removed spot up to the second-to-last
        //    element, right-to-left. This loop "fixes" locked elements that were shifted left
        //    off their anchored positions: If a locked element now has an unlocked element
        //    to its right, swap them back to restore locked order.
        for index in (idx..self.elements.len() - 1).rev() {
            // If current is locked and the next one is not locked, swap them.
            if self.elements[index].locked() && !self.elements[index + 1].locked() {
                self.elements.swap(index, index + 1);
            }
        }

        // 5. Return the removed value.
        Some(removed)
    }

    /// Swaps the elements at indices `i` and `j`, along with their `locked` status, ensuring
    /// the lock state remains associated with the position rather than the element itself.
    pub fn swap_respecting_locks(&mut self, i: T::Index, j: T::Index) {
        let i = i.into_usize();
        let j = j.into_usize();
        self.elements.swap(i, j);
        let locked_i = self.elements[i].locked();
        let locked_j = self.elements[j].locked();
        self.elements[i].set_locked(locked_j);
        self.elements[j].set_locked(locked_i);
    }
}

impl<T: RingElement> Index<T::Index> for Ring<T> {
    type Output = T;
    fn index(&self, index: T::Index) -> &Self::Output {
        &self.elements[index.into_usize()]
    }
}

impl<T: RingElement> IndexMut<T::Index> for Ring<T> {
    fn index_mut(&mut self, index: T::Index) -> &mut Self::Output {
        &mut self.elements[index.into_usize()]
    }
}

impl<'a, T: RingElement> IntoIterator for &'a Ring<T> {
    type Item = &'a T;
    type IntoIter = std::collections::vec_deque::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter()
    }
}

impl<'a, T: RingElement> IntoIterator for &'a mut Ring<T> {
    type Item = &'a mut T;
    type IntoIter = std::collections::vec_deque::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter_mut()
    }
}

impl<T: RingElement> Extend<T> for Ring<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.elements.extend(iter);
    }
}

macro_rules! declare_ring_element_and_index {
    ($container:ident, $index:ident) => {
        impl crate::ring::RingElement for $container {
            type Index = $index;
        }

        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        #[derive(
            Debug,
            Default,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[serde(transparent)]
        #[repr(transparent)]
        pub struct $index {
            index: usize,
        }

        impl std::fmt::Display for $index {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.index)
            }
        }

        impl std::str::FromStr for $index {
            type Err = std::num::ParseIntError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<usize>().map(|index| $index { index })
            }
        }

        impl crate::ring::RingIndex for $index {
            const DEFAULT: $index = $index { index: 0 };

            #[inline]
            fn from_usize(index: usize) -> Self {
                $index { index }
            }
            #[inline]
            fn into_usize(self) -> usize {
                self.index
            }
        }
    };
}

macro_rules! impl_ring_elements {
    ($name:ty, $element:ident) => {
        paste::paste! {
            impl $name {
                pub const fn [<$element:lower s>](&self) -> &Ring<$element> {
                    &self.[<$element:lower s>]
                }

                pub fn [<$element:lower s_mut>](&mut self) -> &mut Ring<$element> {
                    &mut self.[<$element:lower s>]
                }

                #[allow(dead_code)]
                pub fn [<focused_ $element:lower>](&self) -> Option<&$element> {
                    self.[<$element:lower s>].focused()
                }

                pub fn [<focused_ $element:lower _idx>](&self) -> <$element as crate::ring::RingElement>::Index {
                    self.[<$element:lower s>].focused_idx()
                }

                pub fn [<focused_ $element:lower _mut>](&mut self) -> Option<&mut $element> {
                    self.[<$element:lower s>].focused_mut()
                }
            }
        }
    };
    // This allows passing a different name to be used for the functions. For instance, the
    // `floating_windows` ring calls this as:
    // ```rust
    // impl_ring_elements!(Workspace, Window, "floating_window");
    // ```
    // Which allows using the `Window` element but name the functions as `floating_window`
    ($name:ty, $element:ident, $el_name:literal) => {
        paste::paste! {
            impl $name {
                pub const fn [<$el_name:lower s>](&self) -> &Ring<$element> {
                    &self.[<$el_name:lower s>]
                }

                pub fn [<$el_name:lower s_mut>](&mut self) -> &mut Ring<$element> {
                    &mut self.[<$el_name:lower s>]
                }

                #[allow(dead_code)]
                pub fn [<focused_ $el_name:lower>](&self) -> Option<&$element> {
                    self.[<$el_name:lower s>].focused()
                }

                pub fn [<focused_ $el_name:lower _idx>](&self) -> <$element as crate::ring::RingElement>::Index {
                    self.[<$el_name:lower s>].focused_idx()
                }

                pub fn [<focused_ $el_name:lower _mut>](&mut self) -> Option<&mut $element> {
                    self.[<$el_name:lower s>].focused_mut()
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct TestItem {
        val: i32,
        locked: bool,
    }

    declare_ring_element_and_index!(TestItem, TestIdx);

    impl From<usize> for TestIdx {
        fn from(index: usize) -> Self {
            TestIdx { index }
        }
    }

    impl TestItem {
        fn new(val: i32, locked: bool) -> Self {
            TestItem { val, locked }
        }
    }

    impl Lockable for TestItem {
        fn locked(&self) -> bool {
            self.locked
        }

        fn set_locked(&mut self, locked: bool) -> &mut Self {
            self.locked = locked;
            self
        }
    }

    fn test_ring(items: &[(i32, bool)]) -> Ring<TestItem> {
        let mut ring = Ring::default();
        ring.extend(items.iter().map(|&(val, locked)| TestItem { val, locked }));
        ring
    }

    #[test]
    fn test_insert_respecting_locks() {
        // Test case 1: Basic insertion with locked index
        {
            // Lock index 2
            let mut ring = test_ring(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            // Insert at index 0, should shift elements while keeping index 2 locked
            ring.insert_respecting_locks(0.into(), TestItem::new(99, false));
            // Element '2' remains at index 2, element '1' that was at index 1 is now at index 3
            assert_eq!(ring.to_vec(|x| x.val), vec![99, 0, 2, 1, 3, 4]);
        }

        // Test case 2: Insert at a locked index (should insert after locked)
        {
            // Lock index 2
            let mut ring = test_ring(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            // Try to insert at locked index 2, should insert at index 3 instead
            let actual_index = ring.insert_respecting_locks(2.into(), TestItem::new(99, false));
            assert_eq!(actual_index, 3.into());
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 1, 2, 99, 3, 4]);
        }

        // Test case 3: Multiple locked indices
        {
            // Lock index 1 and 3
            let mut ring = test_ring(&[(0, false), (1, true), (2, false), (3, true), (4, false)]);
            // Insert at index 0, should maintain locked indices
            ring.insert_respecting_locks(0.into(), TestItem::new(99, false));
            // Elements '1' and '3' remain at indices 1 and 3
            assert_eq!(ring.to_vec(|x| x.val), vec![99, 1, 0, 3, 2, 4]);
        }

        // Test case 4: Insert at end
        {
            // Lock index 2
            let mut ring = test_ring(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let actual_index = ring.insert_respecting_locks(5.into(), TestItem::new(99, false));
            assert_eq!(actual_index, 5.into());
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 1, 2, 3, 4, 99]);
        }

        // Test case 5: Empty ring
        {
            let mut ring = test_ring(&[]);
            // Insert into empty deque
            let actual_index = ring.insert_respecting_locks(0.into(), TestItem::new(99, false));
            assert_eq!(actual_index, 0.into());
            assert_eq!(ring.to_vec(|x| x.val), vec![99]);
        }

        // Test case 6: All indices locked
        {
            // Lock all indices
            let mut ring = test_ring(&[(0, true), (1, true), (2, true), (3, true), (4, true)]);
            // Try to insert at index 2, should insert at the end
            let actual_index = ring.insert_respecting_locks(2.into(), TestItem::new(99, false));
            assert_eq!(actual_index, 5.into());
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 1, 2, 3, 4, 99]);
        }

        // Test case 7: Consecutive locked indices
        {
            // Lock index 2 and 3
            let mut ring = test_ring(&[(0, false), (1, false), (2, true), (3, true), (4, false)]);
            // Insert at index 1, should maintain consecutive locked indices
            ring.insert_respecting_locks(1.into(), TestItem::new(99, false));
            // Elements '2' and '3' remain at indices 2 and 3
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 99, 2, 3, 1, 4]);
        }
    }

    #[test]
    fn test_remove_respecting_locks() {
        // Test case 1: Remove a non-locked index before a locked index
        {
            // Lock index 2
            let mut ring = test_ring(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(0.into());
            assert_eq!(removed.map(|x| x.val), Some(0));
            // Elements '2' remain at index 2
            assert_eq!(ring.to_vec(|x| x.val), vec![1, 3, 2, 4]);
        }

        // Test case 2: Remove a locked index
        {
            // Lock index 2
            let mut ring = test_ring(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(2.into());
            assert_eq!(removed.map(|x| x.val), Some(2));
            // Elements should stay at the same places
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 1, 3, 4]);
        }

        // Test case 3: Remove an index after a locked index
        {
            // Lock index 1
            let mut ring = test_ring(&[(0, false), (1, true), (2, false), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(3.into());
            assert_eq!(removed.map(|x| x.val), Some(3));
            // Elements should stay at the same places
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 1, 2, 4]);
        }

        // Test case 4: Multiple locked indices
        {
            // Lock index 1 and 3
            let mut ring = test_ring(&[(0, false), (1, true), (2, false), (3, true), (4, false)]);
            let removed = ring.remove_respecting_locks(0.into());
            assert_eq!(removed.map(|x| x.val), Some(0));
            // Elements '1' and '3' remain at indices '1' and '3'
            assert_eq!(ring.to_vec(|x| x.val), vec![2, 1, 4, 3]);
        }

        // Test case 5: Remove the last element
        {
            // Lock index 2
            let mut ring = test_ring(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(4.into());
            assert_eq!(removed.map(|x| x.val), Some(4));
            // Index 2 should still be at the same place
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 1, 2, 3]);
        }

        // Test case 6: Invalid index
        {
            // Lock index 2
            let mut ring = test_ring(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(10.into());
            assert_eq!(removed, None);
            // Deque unchanged
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 1, 2, 3, 4]);
        }

        // Test case 7: Remove enough elements to make a locked index invalid
        {
            // Lock index 2
            let mut ring = test_ring(&[(0, false), (1, false), (2, true)]);
            ring.remove_respecting_locks(0.into());
            // Index 2 should now be '1'
            assert_eq!(ring.to_vec(|x| x.val), vec![1, 2]);
        }

        // Test case 8: Removing an element before multiple locked indices
        {
            // Lock index 2 and 4
            let mut ring = test_ring(&[
                (0, false),
                (1, false),
                (2, true),
                (3, false),
                (4, true),
                (5, false),
            ]);
            let removed = ring.remove_respecting_locks(1.into());
            assert_eq!(removed.map(|x| x.val), Some(1));
            // Both indices should still be at the same place
            assert_eq!(ring.to_vec(|x| x.val), vec![0, 3, 2, 5, 4]);
        }
    }

    #[test]
    fn test_swap_respecting_locks_various_cases() {
        // Swap unlocked and locked
        let mut ring = test_ring(&[(0, false), (1, true), (2, false), (3, false)]);
        ring.swap_respecting_locks(0.into(), 1.into());
        assert_eq!(ring.to_vec(|x| x.val), vec![1, 0, 2, 3]);
        assert_eq!(ring[TestIdx::from_usize(0)].locked, false);
        assert_eq!(ring[TestIdx::from_usize(1)].locked, true);
        ring.swap_respecting_locks(0.into(), 1.into());
        assert_eq!(ring.to_vec(|x| x.val), vec![0, 1, 2, 3]);
        assert_eq!(ring[TestIdx::from_usize(0)].locked, false);
        assert_eq!(ring[TestIdx::from_usize(1)].locked, true);

        // Both locked
        let mut ring = test_ring(&[(0, true), (1, false), (2, true)]);
        ring.swap_respecting_locks(0.into(), 2.into());
        assert_eq!(ring.to_vec(|x| x.val), vec![2, 1, 0]);
        assert!(ring[TestIdx::from_usize(0)].locked);
        assert!(!ring[TestIdx::from_usize(1)].locked);
        assert!(ring[TestIdx::from_usize(2)].locked);

        // Both unlocked
        let mut ring = test_ring(&[(0, false), (1, true), (2, false)]);
        ring.swap_respecting_locks(0.into(), 2.into());
        assert_eq!(ring.to_vec(|x| x.val), vec![2, 1, 0]);
        assert!(!ring[TestIdx::from_usize(0)].locked);
        assert!(ring[TestIdx::from_usize(1)].locked);
        assert!(!ring[TestIdx::from_usize(2)].locked);
    }
}
