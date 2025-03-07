use std::collections::HashSet;
use std::collections::VecDeque;

pub struct LockedDeque<'a, T> {
    deque: &'a mut VecDeque<T>,
    locked_indices: &'a mut HashSet<usize>,
}

impl<'a, T: Clone + PartialEq> LockedDeque<'a, T> {
    pub fn new(deque: &'a mut VecDeque<T>, locked_indices: &'a mut HashSet<usize>) -> Self {
        Self {
            deque,
            locked_indices,
        }
    }

    pub fn insert(&mut self, index: usize, value: T) -> usize {
        insert_respecting_locks(self.deque, self.locked_indices, index, value)
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        remove_respecting_locks(self.deque, self.locked_indices, index)
    }
}

fn insert_respecting_locks<T: Clone>(
    deque: &mut VecDeque<T>,
    locked_indices: &mut HashSet<usize>,
    index: usize,
    value: T,
) -> usize {
    if deque.is_empty() {
        deque.push_back(value);
        return 0;
    }

    // Find the actual insertion point (first unlocked index >= requested index)
    let mut actual_index = index;
    while actual_index < deque.len() && locked_indices.contains(&actual_index) {
        actual_index += 1;
    }

    // If we're inserting at the end, just push_back
    if actual_index >= deque.len() {
        deque.push_back(value);
        return actual_index;
    }

    // Store original values at locked positions
    let locked_values: Vec<(usize, T)> = locked_indices
        .iter()
        .filter_map(|&idx| {
            if idx < deque.len() {
                Some((idx, deque[idx].clone()))
            } else {
                None
            }
        })
        .collect();

    // Store all original values
    let original_values: Vec<T> = deque.iter().cloned().collect();

    // Create a new deque with the correct final size
    let mut new_deque = VecDeque::with_capacity(deque.len() + 1);
    for _ in 0..deque.len() + 1 {
        new_deque.push_back(value.clone()); // Temporary placeholder
    }

    // First, place the new value at the insertion point
    new_deque[actual_index] = value.clone();

    // Then, place all locked values at their original positions
    for (idx, val) in &locked_values {
        new_deque[*idx] = val.clone();
    }

    // Now, fill in all remaining positions with values from the original deque,
    // accounting for the shift caused by insertion
    let mut orig_idx = 0;
    #[allow(clippy::needless_range_loop)]
    for new_idx in 0..new_deque.len() {
        // Skip positions that are already filled (insertion point and locked positions)
        if new_idx == actual_index || locked_indices.contains(&new_idx) {
            continue;
        }

        // Skip original elements that were at locked positions
        while orig_idx < original_values.len() && locked_indices.contains(&orig_idx) {
            orig_idx += 1;
        }

        // If we still have original elements to place
        if orig_idx < original_values.len() {
            new_deque[new_idx] = original_values[orig_idx].clone();
            orig_idx += 1;
        }
    }

    // Update the original deque
    *deque = new_deque;

    actual_index
}

fn remove_respecting_locks<T: Clone>(
    deque: &mut VecDeque<T>,
    locked_indices: &mut HashSet<usize>,
    index: usize,
) -> Option<T> {
    if index >= deque.len() {
        return None;
    }

    let removed = deque[index].clone();

    // If removing a locked index, just remove it and unlock
    if locked_indices.contains(&index) {
        locked_indices.remove(&index);
        deque.remove(index);

        // Update locked indices after the removal point
        let new_locked: HashSet<usize> = locked_indices
            .iter()
            .map(|&idx| if idx > index { idx - 1 } else { idx })
            .collect();
        *locked_indices = new_locked;

        return Some(removed);
    }

    // Let's build a new deque with the correct order
    let mut result = VecDeque::with_capacity(deque.len() - 1);

    // 1. First include all elements before the removal index
    #[allow(clippy::needless_range_loop)]
    for i in 0..index {
        result.push_back(deque[i].clone());
    }

    // 2. Then for each element after the removal index
    #[allow(clippy::needless_range_loop)]
    for i in (index + 1)..deque.len() {
        // If the previous index was locked, we need to swap this element
        // with the previous one in our result
        if locked_indices.contains(&(i - 1)) {
            // Insert this element before the locked element
            if !result.is_empty() {
                let locked_element = result.pop_back().unwrap();
                result.push_back(deque[i].clone());
                result.push_back(locked_element);
            } else {
                // This shouldn't happen with valid inputs
                result.push_back(deque[i].clone());
            }
        } else {
            // Normal case, just add the element
            result.push_back(deque[i].clone());
        }
    }

    // Update the original deque
    *deque = result;

    // Important: Keep the same locked indices (don't update them)
    // Only remove any that are now out of bounds
    locked_indices.retain(|&idx| idx < deque.len());

    Some(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::collections::VecDeque;

    #[test]
    fn test_insert_respecting_locks() {
        // Test case 1: Basic insertion with locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2

            // Insert at index 0, should shift elements while keeping index 2 locked
            insert_respecting_locks(&mut deque, &mut locked, 0, 99);
            assert_eq!(deque, VecDeque::from(vec![99, 0, 2, 1, 3, 4]));
            // Element '2' remains at index 2, element '1' that was at index 1 is now at index 3
        }

        // Test case 2: Insert at a locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2

            // Try to insert at locked index 2, should insert at index 3 instead
            let actual_index = insert_respecting_locks(&mut deque, &mut locked, 2, 99);
            assert_eq!(actual_index, 3);
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 99, 3, 4]));
        }

        // Test case 3: Multiple locked indices
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(1); // Lock index 1
            locked.insert(3); // Lock index 3

            // Insert at index 0, should maintain locked indices
            insert_respecting_locks(&mut deque, &mut locked, 0, 99);
            assert_eq!(deque, VecDeque::from(vec![99, 1, 0, 3, 2, 4]));
            // Elements '1' and '3' remain at indices 1 and 3
        }

        // Test case 4: Insert at end
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2

            // Insert at end of deque
            let actual_index = insert_respecting_locks(&mut deque, &mut locked, 5, 99);
            assert_eq!(actual_index, 5);
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 3, 4, 99]));
        }

        // Test case 5: Empty deque
        {
            let mut deque = VecDeque::new();
            let mut locked = HashSet::new();

            // Insert into empty deque
            let actual_index = insert_respecting_locks(&mut deque, &mut locked, 0, 99);
            assert_eq!(actual_index, 0);
            assert_eq!(deque, VecDeque::from(vec![99]));
        }

        // Test case 6: All indices locked
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            for i in 0..5 {
                locked.insert(i); // Lock all indices
            }

            // Try to insert at index 2, should insert at the end
            let actual_index = insert_respecting_locks(&mut deque, &mut locked, 2, 99);
            assert_eq!(actual_index, 5);
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 3, 4, 99]));
        }

        // Test case 7: Consecutive locked indices
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2
            locked.insert(3); // Lock index 3

            // Insert at index 1, should maintain consecutive locked indices
            insert_respecting_locks(&mut deque, &mut locked, 1, 99);
            assert_eq!(deque, VecDeque::from(vec![0, 99, 2, 3, 1, 4]));
            // Elements '2' and '3' remain at indices 2 and 3
        }
    }

    #[test]
    fn test_remove_respecting_locks() {
        // Test case 1: Remove a non-locked index before a locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2

            let removed = remove_respecting_locks(&mut deque, &mut locked, 0);
            assert_eq!(removed, Some(0));
            assert_eq!(deque, VecDeque::from(vec![1, 3, 2, 4]));
            assert!(locked.contains(&2)); // Index 2 should still be locked
        }

        // Test case 2: Remove a locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2

            let removed = remove_respecting_locks(&mut deque, &mut locked, 2);
            assert_eq!(removed, Some(2));
            assert_eq!(deque, VecDeque::from(vec![0, 1, 3, 4]));
            assert!(!locked.contains(&2)); // Index 2 should be unlocked
        }

        // Test case 3: Remove an index after a locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(1); // Lock index 1

            let removed = remove_respecting_locks(&mut deque, &mut locked, 3);
            assert_eq!(removed, Some(3));
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 4]));
            assert!(locked.contains(&1)); // Index 1 should still be locked
        }

        // Test case 4: Multiple locked indices
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(1); // Lock index 1
            locked.insert(3); // Lock index 3

            let removed = remove_respecting_locks(&mut deque, &mut locked, 0);
            assert_eq!(removed, Some(0));
            assert_eq!(deque, VecDeque::from(vec![2, 1, 4, 3]));
            assert!(locked.contains(&1) && locked.contains(&3)); // Both indices should still be locked
        }

        // Test case 5: Remove the last element
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2

            let removed = remove_respecting_locks(&mut deque, &mut locked, 4);
            assert_eq!(removed, Some(4));
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 3]));
            assert!(locked.contains(&2)); // Index 2 should still be locked
        }

        // Test case 6: Invalid index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2

            let removed = remove_respecting_locks(&mut deque, &mut locked, 10);
            assert_eq!(removed, None);
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 3, 4])); // Deque unchanged
            assert!(locked.contains(&2)); // Lock unchanged
        }

        // Test case 7: Remove enough elements to make a locked index invalid
        {
            let mut deque = VecDeque::from(vec![0, 1, 2]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2

            remove_respecting_locks(&mut deque, &mut locked, 0);
            assert_eq!(deque, VecDeque::from(vec![1, 2]));
            assert!(!locked.contains(&2)); // Index 2 should now be invalid
        }

        // Test case 8: Removing an element before multiple locked indices
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4, 5]);
            let mut locked = HashSet::new();
            locked.insert(2); // Lock index 2
            locked.insert(4); // Lock index 4

            let removed = remove_respecting_locks(&mut deque, &mut locked, 1);
            assert_eq!(removed, Some(1));
            assert_eq!(deque, VecDeque::from(vec![0, 3, 2, 5, 4]));
            assert!(locked.contains(&2) && locked.contains(&4)); // Both indices should still be locked
        }
    }
}
