//! The values the schema says a key has when the file leaves it out.
//!
//! These are the Windows values. Each daemon keeps its own run-time default for the few that
//! differ (the paddings, the border width and offset), and the schema view of a platform says
//! that platform's value (see `schema::platform_changes`).

pub const DEFAULT_RESIZE_DELTA: i32 = 50;
pub const DEFAULT_MOUSE_FOLLOWS_FOCUS: bool = true;
pub const DEFAULT_WORKSPACE_PADDING: i32 = 10;
pub const DEFAULT_CONTAINER_PADDING: i32 = 10;
pub const SLOW_APPLICATION_COMPENSATION_TIME: u64 = 20;
pub const ANIMATION_FPS: u64 = 60;
pub const BORDER_WIDTH: i32 = 8;
pub const BORDER_OFFSET: i32 = -1;
pub const BORDER_ENABLED: bool = true;
pub const BORDER_RADIUS: i32 = 10;
pub const TRANSPARENCY_ENABLED: bool = false;
pub const TRANSPARENCY_ALPHA: u8 = 200;
