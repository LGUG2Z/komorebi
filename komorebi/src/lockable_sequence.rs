use std::collections::VecDeque;

use crate::Lockable;

pub trait LockableSequence<T: Lockable> {
    /// Insert value at idx, keeping locked elements at their absolute positions.
    fn insert_respecting_locks(&mut self, idx: usize, value: T) -> usize;
    /// Remove at idx, keeping locked elements at their absolute positions.
    fn remove_respecting_locks(&mut self, idx: usize) -> Option<T>;
}

impl<T: Lockable> LockableSequence<T> for VecDeque<T> {
    /// Insert `value` at logical index `idx`, with trying to keep locked elements
    /// (`is_locked()`) anchored at their original positions.
    ///
    /// Returns the final index of the inserted element.
    fn insert_respecting_locks(&mut self, mut idx: usize, value: T) -> usize {
        // 1. Bounds check: if index is out of range, simply append.
        if idx >= self.len() {
            self.push_back(value);
            return self.len() - 1; // last index
        }

        // 2. Normal VecDeque insertion
        self.insert(idx, value);

        // 3. Walk left-to-right once, swapping any misplaced locked element. After
        // the VecDeque::insert all items after `idx` have moved right by one. For every locked
        // element that is now to the right of an unlocked one, swap it back left exactly once.
        for index in (idx + 1)..self.len() {
            if self[index].is_locked() && !self[index - 1].is_locked() {
                self.swap(index - 1, index);

                // If the element we just inserted participated in the swap,
                // update `idx` so we can return its final location.
                if idx == index - 1 {
                    idx = index;
                }
            }
        }
        idx
    }

    /// Remove element at `idx`, with trying to keep locked elements
    /// (`is_locked()`) anchored at their original positions.
    ///
    /// Returns the removed element, or `None` if `idx` is out of bounds.
    fn remove_respecting_locks(&mut self, idx: usize) -> Option<T> {
        // 1. Bounds check: if index is out of range, do nothing.
        if idx >= self.len() {
            return None;
        }

        // 2. Remove the element at the requested index.
        //    All elements after idx are now shifted left by 1.
        let removed = self.remove(idx)?;

        // 3. If less than 2 elements remain, nothing to shift.
        if self.len() < 2 {
            return Some(removed);
        }

        // 4. Iterate from the element just after the removed spot up to the second-to-last
        //    element, right-to-left. This loop "fixes" locked elements that were shifted left
        //    off their anchored positions: If a locked element now has an unlocked element
        //    to its right, swap them back to restore locked order.
        for index in (idx..self.len() - 1).rev() {
            // If current is locked and the next one is not locked, swap them.
            if self[index].is_locked() && !self[index + 1].is_locked() {
                self.swap(index, index + 1);
            }
        }

        // 5. Return the removed value.
        Some(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct TestItem {
        val: i32,
        locked: bool,
    }

    impl Lockable for TestItem {
        fn is_locked(&self) -> bool {
            self.locked
        }
    }

    fn vals(v: &VecDeque<TestItem>) -> Vec<i32> {
        v.iter().map(|x| x.val).collect()
    }

    fn test_deque(items: &[(i32, bool)]) -> VecDeque<TestItem> {
        items
            .iter()
            .cloned()
            .map(|(val, locked)| TestItem { val, locked })
            .collect()
    }

    #[test]
    fn test_insert_respecting_locks() {
        // Test case 1: Basic insertion with locked index
        {
            // Lock index 2
            let mut ring = test_deque(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            // Insert at index 0, should shift elements while keeping index 2 locked
            ring.insert_respecting_locks(
                0,
                TestItem {
                    val: 99,
                    locked: false,
                },
            );
            // Element '2' remains at index 2, element '1' that was at index 1 is now at index 3
            assert_eq!(vals(&ring), vec![99, 0, 2, 1, 3, 4]);
        }

        // Test case 2: Insert at a locked index (should insert after locked)
        {
            // Lock index 2
            let mut ring = test_deque(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            // Try to insert at locked index 2, should insert at index 3 instead
            let actual_index = ring.insert_respecting_locks(
                2,
                TestItem {
                    val: 99,
                    locked: false,
                },
            );
            assert_eq!(actual_index, 3);
            assert_eq!(vals(&ring), vec![0, 1, 2, 99, 3, 4]);
        }

        // Test case 3: Multiple locked indices
        {
            // Lock index 1 and 3
            let mut ring = test_deque(&[(0, false), (1, true), (2, false), (3, true), (4, false)]);
            // Insert at index 0, should maintain locked indices
            ring.insert_respecting_locks(
                0,
                TestItem {
                    val: 99,
                    locked: false,
                },
            );
            // Elements '1' and '3' remain at indices 1 and 3
            assert_eq!(vals(&ring), vec![99, 1, 0, 3, 2, 4]);
        }

        // Test case 4: Insert at end
        {
            // Lock index 2
            let mut ring = test_deque(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let actual_index = ring.insert_respecting_locks(
                5,
                TestItem {
                    val: 99,
                    locked: false,
                },
            );
            assert_eq!(actual_index, 5);
            assert_eq!(vals(&ring), vec![0, 1, 2, 3, 4, 99]);
        }

        // Test case 5: Empty ring
        {
            let mut ring = test_deque(&[]);
            // Insert into empty deque
            let actual_index = ring.insert_respecting_locks(
                0,
                TestItem {
                    val: 99,
                    locked: false,
                },
            );
            assert_eq!(actual_index, 0);
            assert_eq!(vals(&ring), vec![99]);
        }

        // Test case 6: All indices locked
        {
            // Lock all indices
            let mut ring = test_deque(&[(0, true), (1, true), (2, true), (3, true), (4, true)]);
            // Try to insert at index 2, should insert at the end
            let actual_index = ring.insert_respecting_locks(
                2,
                TestItem {
                    val: 99,
                    locked: false,
                },
            );
            assert_eq!(actual_index, 5);
            assert_eq!(vals(&ring), vec![0, 1, 2, 3, 4, 99]);
        }

        // Test case 7: Consecutive locked indices
        {
            // Lock index 2 and 3
            let mut ring = test_deque(&[(0, false), (1, false), (2, true), (3, true), (4, false)]);
            // Insert at index 1, should maintain consecutive locked indices
            ring.insert_respecting_locks(
                1,
                TestItem {
                    val: 99,
                    locked: false,
                },
            );
            // Elements '2' and '3' remain at indices 2 and 3
            assert_eq!(vals(&ring), vec![0, 99, 2, 3, 1, 4]);
        }
    }

    #[test]
    fn test_remove_respecting_locks() {
        // Test case 1: Remove a non-locked index before a locked index
        {
            // Lock index 2
            let mut ring = test_deque(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(0);
            assert_eq!(removed.map(|x| x.val), Some(0));
            // Elements '2' remain at index 2
            assert_eq!(vals(&ring), vec![1, 3, 2, 4]);
        }

        // Test case 2: Remove a locked index
        {
            // Lock index 2
            let mut ring = test_deque(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(2);
            assert_eq!(removed.map(|x| x.val), Some(2));
            // Elements should stay at the same places
            assert_eq!(vals(&ring), vec![0, 1, 3, 4]);
        }

        // Test case 3: Remove an index after a locked index
        {
            // Lock index 1
            let mut ring = test_deque(&[(0, false), (1, true), (2, false), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(3);
            assert_eq!(removed.map(|x| x.val), Some(3));
            // Elements should stay at the same places
            assert_eq!(vals(&ring), vec![0, 1, 2, 4]);
        }

        // Test case 4: Multiple locked indices
        {
            // Lock index 1 and 3
            let mut ring = test_deque(&[(0, false), (1, true), (2, false), (3, true), (4, false)]);
            let removed = ring.remove_respecting_locks(0);
            assert_eq!(removed.map(|x| x.val), Some(0));
            // Elements '1' and '3' remain at indices '1' and '3'
            assert_eq!(vals(&ring), vec![2, 1, 4, 3]);
        }

        // Test case 5: Remove the last element
        {
            // Lock index 2
            let mut ring = test_deque(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(4);
            assert_eq!(removed.map(|x| x.val), Some(4));
            // Index 2 should still be at the same place
            assert_eq!(vals(&ring), vec![0, 1, 2, 3]);
        }

        // Test case 6: Invalid index
        {
            // Lock index 2
            let mut ring = test_deque(&[(0, false), (1, false), (2, true), (3, false), (4, false)]);
            let removed = ring.remove_respecting_locks(10);
            assert_eq!(removed, None);
            // Deque unchanged
            assert_eq!(vals(&ring), vec![0, 1, 2, 3, 4]);
        }

        // Test case 7: Remove enough elements to make a locked index invalid
        {
            // Lock index 2
            let mut ring = test_deque(&[(0, false), (1, false), (2, true)]);
            ring.remove_respecting_locks(0);
            // Index 2 should now be '1'
            assert_eq!(vals(&ring), vec![1, 2]);
        }

        // Test case 8: Removing an element before multiple locked indices
        {
            // Lock index 2 and 4
            let mut ring = test_deque(&[
                (0, false),
                (1, false),
                (2, true),
                (3, false),
                (4, true),
                (5, false),
            ]);
            let removed = ring.remove_respecting_locks(1);
            assert_eq!(removed.map(|x| x.val), Some(1));
            // Both indices should still be at the same place
            assert_eq!(vals(&ring), vec![0, 3, 2, 5, 4]);
        }
    }
}
