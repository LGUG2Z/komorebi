use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub static ANIMATIONS_IN_PROGRESS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy)]
struct AnimationState {
    pub in_progress: bool,
    pub is_cancelled: bool,
}

#[derive(Debug)]
pub struct AnimationManager {
    animations: HashMap<isize, AnimationState>,
}

impl Default for AnimationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationManager {
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
        }
    }

    pub fn is_cancelled(&self, hwnd: isize) -> bool {
        if let Some(animation_state) = self.animations.get(&hwnd) {
            animation_state.is_cancelled
        } else {
            false
        }
    }

    pub fn in_progress(&self, hwnd: isize) -> bool {
        if let Some(animation_state) = self.animations.get(&hwnd) {
            animation_state.in_progress
        } else {
            false
        }
    }

    pub fn cancel(&mut self, hwnd: isize) {
        if let Some(animation_state) = self.animations.get_mut(&hwnd) {
            animation_state.is_cancelled = true;
        }
    }

    pub fn start(&mut self, hwnd: isize) {
        if let Entry::Vacant(e) = self.animations.entry(hwnd) {
            e.insert(AnimationState {
                in_progress: true,
                is_cancelled: false,
            });

            ANIMATIONS_IN_PROGRESS.store(self.animations.len(), Ordering::Release);
            return;
        }

        if let Some(animation_state) = self.animations.get_mut(&hwnd) {
            animation_state.in_progress = true;
        }
    }

    pub fn end(&mut self, hwnd: isize) {
        if let Some(animation_state) = self.animations.get_mut(&hwnd) {
            animation_state.in_progress = false;
            animation_state.is_cancelled = false;

            self.animations.remove(&hwnd);
            ANIMATIONS_IN_PROGRESS.store(self.animations.len(), Ordering::Release);
        }
    }
}
