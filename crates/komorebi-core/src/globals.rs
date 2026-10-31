//! [`WorkspaceGlobals`], what a monitor gives the workspaces it shows.

use komorebi_config::behaviour::FloatingLayerBehaviour;
use komorebi_layouts::Rect;
use serde::Deserialize;
use serde::Serialize;

/// The work area a monitor gives the workspace it shows, the padding and the offset the workspace
/// falls back to when it sets none of its own, the monitor's window-based offset, and the room
/// around each window that a border takes.
///
/// Set by the parent monitor or by the window manager, never read from a file: a workspace skips
/// it when it is serialised.
#[derive(Debug, Default, Copy, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WorkspaceGlobals {
    /// The container padding of the monitor.
    pub container_padding: Option<i32>,
    /// The workspace padding of the monitor.
    pub workspace_padding: Option<i32>,
    /// The width of the border around each window, 0 when none is drawn.
    pub border_width: i32,
    /// The offset of the border from the window.
    pub border_offset: i32,
    /// The work area of the monitor.
    pub work_area: Rect,
    /// The offset of the work area, if any.
    pub work_area_offset: Option<Rect>,
    /// The offset the monitor has asked for according to its windows.
    pub window_based_work_area_offset: Option<Rect>,
    /// The most windows the window-based offset is applied for.
    pub window_based_work_area_offset_limit: isize,
    /// The floating layer behaviour of the monitor, which the workspace's own overrides
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_globals_ask_for_nothing() {
        let globals = WorkspaceGlobals::default();

        assert_eq!(globals.container_padding, None);
        assert_eq!(globals.border_width, 0);
        assert_eq!(globals.work_area, Rect::default());
        assert_eq!(globals.window_based_work_area_offset_limit, 0);
    }

    #[test]
    fn globals_are_a_copy_that_compares_by_value() {
        let a = WorkspaceGlobals {
            border_width: 8,
            ..Default::default()
        };
        let b = a;

        assert_eq!(a, b);
    }
}
