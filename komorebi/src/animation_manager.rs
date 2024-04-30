use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
struct AnimationState {
    pub in_progress: bool,
    pub is_cancelled: bool,
}

#[derive(Debug)]
pub struct AnimationManager {
    animations: HashMap<isize, AnimationState>,
}

impl AnimationManager {
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
        }
    }

    pub fn is_cancelled(&self, hwnd: isize) -> bool {
        if !self.animations.contains_key(&hwnd) {
            return false;
        }

        self.animations.get(&hwnd).unwrap().is_cancelled
    }

    pub fn in_progress(&self, hwnd: isize) -> bool {
        if !self.animations.contains_key(&hwnd) {
            return false;
        }

        self.animations.get(&hwnd).unwrap().in_progress
    }

    pub fn cancel(&mut self, hwnd: isize) {
        if !self.animations.contains_key(&hwnd) {
            return;
        }

        let state = self.animations.get_mut(&hwnd).unwrap();
        state.is_cancelled = true;
    }

    pub fn start(&mut self, hwnd: isize) {
        if !self.animations.contains_key(&hwnd) {
            self.animations.insert(
                hwnd,
                AnimationState {
                    in_progress: true,
                    is_cancelled: false,
                },
            );
            return;
        }

        let state = self.animations.get_mut(&hwnd).unwrap();

        if !state.in_progress {
            state.in_progress = true;
        }
    }

    pub fn end(&mut self, hwnd: isize) {
        if !self.animations.contains_key(&hwnd) {
            return;
        }

        let state = self.animations.get_mut(&hwnd).unwrap();
        state.in_progress = false;
        state.is_cancelled = false;

        self.animations.remove(&hwnd);
    }
}
