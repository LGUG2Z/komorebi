use std::collections::VecDeque;

#[derive(Debug, Clone)]
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
