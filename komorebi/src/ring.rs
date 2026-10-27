//! [`Ring`], a list of elements with one of them focused.
//!
//! Monitors, workspaces and containers each keep their children in a ring, so that "the focused
//! one" is the same thing everywhere, and `impl_ring_elements!` gives each of them the same
//! accessors for it.

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

/// Gives a type that keeps its children in a [`Ring`] the accessors every such type has.
///
/// `impl_ring_elements!(Workspace, Container)` reads the field `containers: Ring<Container>` and
/// gives `Workspace` the methods `containers()`, `containers_mut()`, `focused_container()`,
/// `focused_container_idx()` and `focused_container_mut()`. The field is named after the element
/// type in lower case with an `s` appended, and the methods after the field, so that "the focused
/// container" is spelled the same way on every type that has one.
macro_rules! impl_ring_elements {
    ($name:ty, $element:ident) => {
        pastey::paste! {
            impl $name {
                pub const fn [<$element:lower s>](&self) -> &std::collections::VecDeque<$element> {
                    self.[<$element:lower s>].elements()
                }

                pub fn [<$element:lower s_mut>](&mut self) -> &mut std::collections::VecDeque<$element> {
                    self.[<$element:lower s>].elements_mut()
                }

                #[allow(dead_code)]
                pub fn [<focused_ $element:lower>](&self) -> Option<&$element> {
                    self.[<$element:lower s>].focused()
                }

                pub const fn [<focused_ $element:lower _idx>](&self) -> usize {
                    self.[<$element:lower s>].focused_idx()
                }

                pub fn [<focused_ $element:lower _mut>](&mut self) -> Option<&mut $element> {
                    self.[<$element:lower s>].focused_mut()
                }
            }
        }
    };    // This allows passing a different name to be used for the functions. For instance, the
    // `floating_windows` ring calls this as:
    // ```rust
    // impl_ring_elements!(Workspace, Window, "floating_window");
    // ```
    // Which allows using the `Window` element but name the functions as `floating_window`
    ($name:ty, $element:ident, $el_name:literal) => {
        pastey::paste! {
            impl $name {
                pub const fn [<$el_name:lower s>](&self) -> &std::collections::VecDeque<$element> {
                    self.[<$el_name:lower s>].elements()
                }

                pub fn [<$el_name:lower s_mut>](&mut self) -> &mut std::collections::VecDeque<$element> {
                    self.[<$el_name:lower s>].elements_mut()
                }

                #[allow(dead_code)]
                pub fn [<focused_ $el_name:lower>](&self) -> Option<&$element> {
                    self.[<$el_name:lower s>].focused()
                }

                pub const fn [<focused_ $el_name:lower _idx>](&self) -> usize {
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
    struct Book(&'static str);

    struct Shelf {
        books: Ring<Book>,
    }

    impl_ring_elements!(Shelf, Book);

    #[test]
    fn impl_ring_elements_names_the_accessors_after_the_element() {
        let mut shelf = Shelf {
            books: Ring::default(),
        };
        shelf.books_mut().extend([Book("a"), Book("b")]);
        shelf.books.focus(1);

        assert_eq!(shelf.books().len(), 2);
        assert_eq!(shelf.focused_book(), Some(&Book("b")));
        assert_eq!(shelf.focused_book_idx(), 1);

        if let Some(book) = shelf.focused_book_mut() {
            *book = Book("c");
        }

        assert_eq!(shelf.books()[1], Book("c"));
    }

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
