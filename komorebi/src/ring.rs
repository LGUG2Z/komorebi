use std::collections::vec_deque::IntoIter;
use std::collections::VecDeque;

use serde::Deserialize;
use serde::Serialize;

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

impl<T> IntoIterator for Ring<T> {
    type Item = T;
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.into_iter()
    }
}

impl<T> Ring<T> {
    pub const fn elements(&self) -> &VecDeque<T> {
        &self.elements
    }

    pub fn elements_mut(&mut self) -> &mut VecDeque<T> {
        &mut self.elements
    }

    pub fn focus(&mut self, idx: usize) {
        self.focused = idx;
    }

    pub fn focused(&self) -> Option<&T> {
        self.elements.get(self.focused)
    }

    pub const fn focused_idx(&self) -> usize {
        self.focused
    }

    pub fn focused_mut(&mut self) -> Option<&mut T> {
        self.elements.get_mut(self.focused)
    }

    pub fn swap(&mut self, i: usize, j: usize) {
        self.elements.swap(i, j);
    }
}

macro_rules! impl_ring_elements {
    ($name:ty, $element:ident) => {
        paste::paste! {
            impl $name {
                pub const fn [<$element:lower s>](&self) -> &VecDeque<$element> {
                    self.[<$element:lower s>].elements()
                }

                pub fn [<$element:lower s_mut>](&mut self) -> &mut VecDeque<$element> {
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
                pub const fn [<$el_name:lower s>](&self) -> &VecDeque<$element> {
                    self.[<$el_name:lower s>].elements()
                }

                pub fn [<$el_name:lower s_mut>](&mut self) -> &mut VecDeque<$element> {
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
