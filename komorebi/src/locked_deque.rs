use std::collections::BTreeSet;
use std::collections::VecDeque;

pub struct LockedDeque<'a, T> {
    deque: &'a mut VecDeque<T>,
    locked_indices: &'a mut BTreeSet<usize>,
}

impl<'a, T: PartialEq> LockedDeque<'a, T> {
    pub fn new(deque: &'a mut VecDeque<T>, locked_indices: &'a mut BTreeSet<usize>) -> Self {
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

pub fn insert_respecting_locks<T>(
    deque: &mut VecDeque<T>,
    locked_idx: &mut BTreeSet<usize>,
    idx: usize,
    value: T,
) -> usize {
    if idx == deque.len() {
        deque.push_back(value);
        return idx;
    }

    let mut new_deque = VecDeque::with_capacity(deque.len() + 1);
    let mut temp_locked_deque = VecDeque::new();
    let mut j = 0;
    let mut corrected_idx = idx;

    for (i, el) in deque.drain(..).enumerate() {
        if i == idx {
            corrected_idx = j;
        }
        if locked_idx.contains(&i) {
            temp_locked_deque.push_back(el);
        } else {
            new_deque.push_back(el);
            j += 1;
        }
    }

    new_deque.insert(corrected_idx, value);

    for (locked_el, locked_idx) in temp_locked_deque.into_iter().zip(locked_idx.iter()) {
        new_deque.insert(*locked_idx, locked_el);
        if *locked_idx <= corrected_idx {
            corrected_idx += 1;
        }
    }

    *deque = new_deque;

    corrected_idx
}

pub fn remove_respecting_locks<T>(
    deque: &mut VecDeque<T>,
    locked_idx: &mut BTreeSet<usize>,
    idx: usize,
) -> Option<T> {
    if idx >= deque.len() {
        return None;
    }

    let final_size = deque.len() - 1;

    let mut new_deque = VecDeque::with_capacity(final_size);
    let mut temp_locked_deque = VecDeque::new();
    let mut removed = None;
    let mut removed_locked_idx = None;

    for (i, el) in deque.drain(..).enumerate() {
        if i == idx {
            removed = Some(el);
            removed_locked_idx = locked_idx.contains(&i).then_some(i);
        } else if locked_idx.contains(&i) {
            temp_locked_deque.push_back(el);
        } else {
            new_deque.push_back(el);
        }
    }

    if let Some(i) = removed_locked_idx {
        let mut above = locked_idx.split_off(&i);
        above.pop_first();
        locked_idx.extend(above.into_iter().map(|i| i - 1));
    }

    while locked_idx.last().is_some_and(|i| *i >= final_size) {
        locked_idx.pop_last();
    }

    let extra_invalid_idx = (new_deque.len()
        ..(new_deque.len() + temp_locked_deque.len() - locked_idx.len()))
        .collect::<Vec<_>>();

    for (locked_el, locked_idx) in temp_locked_deque
        .into_iter()
        .zip(locked_idx.iter().chain(extra_invalid_idx.iter()))
    {
        new_deque.insert(*locked_idx, locked_el);
    }

    *deque = new_deque;

    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::collections::VecDeque;

    #[test]
    fn test_insert_respecting_locks() {
        // Test case 1: Basic insertion with locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2

            // Insert at index 0, should shift elements while keeping index 2 locked
            insert_respecting_locks(&mut deque, &mut locked, 0, 99);
            assert_eq!(deque, VecDeque::from(vec![99, 0, 2, 1, 3, 4]));
            // Element '2' remains at index 2, element '1' that was at index 1 is now at index 3
        }

        // Test case 2: Insert at a locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2

            // Try to insert at locked index 2, should insert at index 3 instead
            let actual_index = insert_respecting_locks(&mut deque, &mut locked, 2, 99);
            assert_eq!(actual_index, 3);
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 99, 3, 4]));
        }

        // Test case 3: Multiple locked indices
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = BTreeSet::new();
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
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2

            // Insert at end of deque
            let actual_index = insert_respecting_locks(&mut deque, &mut locked, 5, 99);
            assert_eq!(actual_index, 5);
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 3, 4, 99]));
        }

        // Test case 5: Empty deque
        {
            let mut deque = VecDeque::new();
            let mut locked = BTreeSet::new();

            // Insert into empty deque
            let actual_index = insert_respecting_locks(&mut deque, &mut locked, 0, 99);
            assert_eq!(actual_index, 0);
            assert_eq!(deque, VecDeque::from(vec![99]));
        }

        // Test case 6: All indices locked
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = BTreeSet::new();
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
            let mut locked = BTreeSet::new();
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
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2

            let removed = remove_respecting_locks(&mut deque, &mut locked, 0);
            assert_eq!(removed, Some(0));
            assert_eq!(deque, VecDeque::from(vec![1, 3, 2, 4]));
            assert!(locked.contains(&2)); // Index 2 should still be locked
        }

        // Test case 2: Remove a locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2

            let removed = remove_respecting_locks(&mut deque, &mut locked, 2);
            assert_eq!(removed, Some(2));
            assert_eq!(deque, VecDeque::from(vec![0, 1, 3, 4]));
            assert!(!locked.contains(&2)); // Index 2 should be unlocked
        }

        // Test case 3: Remove an index after a locked index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = BTreeSet::new();
            locked.insert(1); // Lock index 1

            let removed = remove_respecting_locks(&mut deque, &mut locked, 3);
            assert_eq!(removed, Some(3));
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 4]));
            assert!(locked.contains(&1)); // Index 1 should still be locked
        }

        // Test case 4: Multiple locked indices
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = BTreeSet::new();
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
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2

            let removed = remove_respecting_locks(&mut deque, &mut locked, 4);
            assert_eq!(removed, Some(4));
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 3]));
            assert!(locked.contains(&2)); // Index 2 should still be locked
        }

        // Test case 6: Invalid index
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4]);
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2

            let removed = remove_respecting_locks(&mut deque, &mut locked, 10);
            assert_eq!(removed, None);
            assert_eq!(deque, VecDeque::from(vec![0, 1, 2, 3, 4])); // Deque unchanged
            assert!(locked.contains(&2)); // Lock unchanged
        }

        // Test case 7: Remove enough elements to make a locked index invalid
        {
            let mut deque = VecDeque::from(vec![0, 1, 2]);
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2

            remove_respecting_locks(&mut deque, &mut locked, 0);
            assert_eq!(deque, VecDeque::from(vec![1, 2]));
            assert!(!locked.contains(&2)); // Index 2 should now be invalid
        }

        // Test case 8: Removing an element before multiple locked indices
        {
            let mut deque = VecDeque::from(vec![0, 1, 2, 3, 4, 5]);
            let mut locked = BTreeSet::new();
            locked.insert(2); // Lock index 2
            locked.insert(4); // Lock index 4

            let removed = remove_respecting_locks(&mut deque, &mut locked, 1);
            assert_eq!(removed, Some(1));
            assert_eq!(deque, VecDeque::from(vec![0, 3, 2, 5, 4]));
            assert!(locked.contains(&2) && locked.contains(&4)); // Both indices should still be locked
        }
    }
}
