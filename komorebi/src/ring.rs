//! [`Ring`], a list of elements with one of them focused.
//!
//! Monitors, workspaces and containers each keep their children in a ring, so that "the focused
//! one" is the same thing everywhere.

use std::collections::VecDeque;

use serde::Deserialize;
use serde::Serialize;

/// A list of elements, one of which is focused.
///
/// The focused index is not guaranteed to be in range: [`Ring::focus`] stores what it is given, and
/// [`Ring::focused`] and [`Ring::focused_mut`] return `None` when it is out of range, as it is for
/// an empty ring.
///
/// # Example
///
/// ```rust
/// use komorebi::ring::Ring;
///
/// let mut ring = Ring::default();
/// ring.elements_mut().push_back("a");
/// ring.elements_mut().push_back("b");
/// ring.focus(1);
///
/// assert_eq!(ring.focused(), Some(&"b"));
/// assert_eq!(ring.focused_idx(), 1);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Ring<T> {
    elements: VecDeque<T>,
    focused: usize,
}

impl<T> Default for Ring<T> {
    fn default() -> Self {
        Self {
            elements: VecDeque::default(),
            focused: 0,
        }
    }
}

impl<T> Ring<T> {
    /// The elements, in order.
    pub const fn elements(&self) -> &VecDeque<T> {
        &self.elements
    }

    /// The elements, in order, to add to or remove from.
    pub fn elements_mut(&mut self) -> &mut VecDeque<T> {
        &mut self.elements
    }

    /// Focuses the element at `idx`.
    pub fn focus(&mut self, idx: usize) {
        self.focused = idx;
    }

    /// The focused element, if the focused index is in range.
    pub fn focused(&self) -> Option<&T> {
        self.elements.get(self.focused)
    }

    /// The index that is focused, in range or not.
    pub const fn focused_idx(&self) -> usize {
        self.focused
    }

    /// The focused element, to change, if the focused index is in range.
    pub fn focused_mut(&mut self) -> Option<&mut T> {
        self.elements.get_mut(self.focused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_ring_focuses_nothing() {
        let mut ring: Ring<u8> = Ring::default();

        assert_eq!(ring.focused(), None);
        assert_eq!(ring.focused_mut(), None);
        assert_eq!(ring.focused_idx(), 0);
    }

    #[test]
    fn focus_selects_the_element_at_that_index() {
        let mut ring = Ring::default();
        ring.elements_mut().extend([10, 20, 30]);

        ring.focus(2);

        assert_eq!(ring.focused(), Some(&30));
        assert_eq!(ring.focused_idx(), 2);
    }

    #[test]
    fn the_focused_element_can_be_changed_in_place() {
        let mut ring = Ring::default();
        ring.elements_mut().extend([1, 2]);
        ring.focus(1);

        if let Some(element) = ring.focused_mut() {
            *element = 5;
        }

        assert_eq!(ring.elements(), &[1, 5]);
    }

    #[test]
    fn a_focused_index_past_the_end_focuses_nothing() {
        let mut ring = Ring::default();
        ring.elements_mut().push_back('a');

        ring.focus(3);

        assert_eq!(ring.focused(), None);
        assert_eq!(ring.focused_idx(), 3);
    }

    #[test]
    fn a_ring_round_trips_through_json() {
        let mut ring = Ring::default();
        ring.elements_mut()
            .extend(["a".to_string(), "b".to_string()]);
        ring.focus(1);

        let json = serde_json::to_string(&ring).expect("serializes");
        let back: Ring<String> = serde_json::from_str(&json).expect("deserializes");

        assert_eq!(back, ring);
    }
}
