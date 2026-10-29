//! Moving the strip of the Scrolling layout across the screen without changing which window has
//! focus, by a step, which is what this has been asked for so far.

use crate::core::PanDirection;
use crate::window_manager::WindowManager;
use color_eyre::eyre;

impl WindowManager {
    /// Move the view of the focused workspace one step, a column, to the left or the right, and
    /// let it come to rest with a column against an edge of the screen. Focus goes to the nearest column in view if the focused one is
    /// not. Nothing happens in a layout that is not Scrolling.
    #[tracing::instrument(skip(self))]
    pub fn pan(&mut self, direction: PanDirection) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;

        if workspace.begin_scrolling_pan().is_none() {
            return Ok(());
        }

        let step = workspace
            .scrolling_pan_step()
            .and_then(|step| u32::try_from(step).ok())
            .unwrap_or_default();

        workspace.pan_scrolling(direction.strip_offset(step));
        let focus_moved = workspace.end_scrolling_pan(0.0);

        self.update_focused_workspace(focus_moved && self.mouse_follows_focus, focus_moved)
    }
}
