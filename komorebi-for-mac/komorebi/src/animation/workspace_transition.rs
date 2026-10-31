//! Switching workspaces with the windows of both sliding across the monitor
//!
//! Pictures are taken of the windows that slide, the real windows are put away or kept waiting
//! behind them, and when the pictures have slid the windows that are entered are shown where the
//! pictures ended up and the pictures fade away over them.
//!
//! The window manager is held for the whole of it, which is as long as the animation lasts. What
//! other threads ask of it waits, and nothing else can move a window that is waiting to be shown.

use crate::animation::capture::capture;
use crate::animation::ghost::Ghost;
use crate::animation::ghost::Overlay;
use crate::animation::step::MotionSpec;
use crate::animation::transition::Plan;
use crate::animation::transition::Slide;
use crate::border_manager::BorderLook;
use color_eyre::eyre;
use std::collections::HashMap;
use std::time::Duration;

/// How long the pictures take to fade away over the windows that took their place
const FADE: Duration = Duration::from_millis(120);

pub struct WorkspaceTransition {
    plan: Plan,
    ghosts: Vec<Ghost>,
    motion: MotionSpec,
}

impl WorkspaceTransition {
    /// Take the pictures that `plan` needs, with the borders that go around the windows in them
    ///
    /// A window that cannot be pictured is not slid, and is put away or shown at once like it is
    /// when nothing is animated. This is `None` when there is nothing left to slide.
    pub fn capture(
        plan: Plan,
        motion: MotionSpec,
        borders: &HashMap<u32, BorderLook>,
    ) -> Option<Self> {
        let mut ghosts = vec![];

        for Slide { id, from, to } in plan.exits.iter().chain(plan.enters.iter()) {
            match capture(*id) {
                Some(still) => ghosts.push(Ghost {
                    still,
                    border: borders.get(id).copied(),
                    from: *from,
                    to: *to,
                }),
                None => tracing::debug!("workspace transition: no picture of window {id}"),
            }
        }

        if ghosts.is_empty() {
            return None;
        }

        Some(Self {
            plan,
            ghosts,
            motion,
        })
    }

    /// Slide the pictures, with `hide` putting the workspace that is left away once they are over
    /// it and `restore` showing the workspace that is entered once they have slid
    ///
    /// Both are handed `subject`, which they cannot both borrow otherwise. Both are done whatever happens to the pictures, and an error of either is returned once
    /// both have been done.
    pub fn run<T>(
        self,
        subject: &mut T,
        hide: impl FnOnce(&mut T) -> eyre::Result<()>,
        restore: impl FnOnce(&mut T) -> eyre::Result<()>,
    ) -> eyre::Result<()> {
        let Some(overlay) = Overlay::show(self.plan.clip, self.ghosts) else {
            tracing::warn!("workspace transition: could not put the pictures on screen");
            let hidden = hide(subject);
            let restored = restore(subject);

            return hidden.and(restored);
        };

        let hidden = hide(subject);

        if hidden.is_ok() {
            overlay.slide(self.motion.duration, self.motion.style);
        }

        let restored = restore(subject);

        if hidden.is_ok() && restored.is_ok() {
            overlay.fade_out(FADE);
        } else {
            overlay.close();
        }

        hidden.and(restored)
    }
}
