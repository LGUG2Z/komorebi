use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub static ANIMATIONS_IN_PROGRESS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy)]
struct AnimationState {
    pub in_progress: bool,
    pub cancelled_count: usize,
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
            animation_state.cancelled_count > 0
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

    pub fn init_cancel(&mut self, hwnd: isize) -> usize {
        if let Some(animation_state) = self.animations.get_mut(&hwnd) {
            animation_state.cancelled_count += 1;
            animation_state.cancelled_count
        } else {
            0
        }
    }

    pub fn end_cancel(&mut self, hwnd: isize) -> usize {
        if let Some(animation_state) = self.animations.get_mut(&hwnd) {
            let cancelled_count = animation_state.cancelled_count;
            animation_state.cancelled_count -= 1;

            cancelled_count
        } else {
            0
        }
    }

    pub fn cancel(&mut self, hwnd: isize) {
        if let Some(animation_state) = self.animations.get_mut(&hwnd) {
            animation_state.in_progress = false;
        }
    }

    pub fn start(&mut self, hwnd: isize) {
        if let Entry::Vacant(e) = self.animations.entry(hwnd) {
            e.insert(AnimationState {
                in_progress: true,
                cancelled_count: 0,
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

            if animation_state.cancelled_count == 0 {
                self.animations.remove(&hwnd);
                ANIMATIONS_IN_PROGRESS.store(self.animations.len(), Ordering::Release);
            }
        }
    }
}
