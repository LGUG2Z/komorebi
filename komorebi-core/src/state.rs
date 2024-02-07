use std::collections::{HashMap, VecDeque};

use serde::Deserialize;

use crate::{config_generation::IdWithIdentifier, Axis, FocusFollowsMouseImplementation, Layout, MoveBehaviour, WindowContainerBehaviour};


#[derive(Debug, Default, Clone, Copy, Deserialize, Eq, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Window {
    pub hwnd: isize,
    pub title: String,
    pub exe: String,
    pub class: String,
    pub rect: Rect
}
#[derive(Debug, Clone, Deserialize)]
pub struct Ring<T> {
    pub elements: VecDeque<T>,
    pub focused: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Container {
    pub windows: Ring<Window>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub name: Option<String>,
    pub containers: Ring<Container>,
    pub monocle_container: Option<Container>,
    pub maximized_window: Option<Window>,
    pub floating_windows: Vec<Window>,
    pub layout: Layout,
    pub layout_rules: Vec<(usize, Layout)>,
    pub layout_flip: Option<Axis>,
    pub workspace_padding: Option<i32>,
    pub container_padding: Option<i32>,
    pub resize_dimensions: Vec<Option<Rect>>,
    pub tile: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Monitor {
    pub id: isize,
    pub name: String,
    pub device: Option<String>,
    pub device_id: Option<String>,
    pub size: Rect,
    pub work_area_size: Rect,
    pub work_area_offset: Option<Rect>,
    pub workspaces: Ring<Workspace>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Deserialize)]
pub struct State {
    pub monitors: Ring<Monitor>,
    pub is_paused: bool,
    pub invisible_borders: Rect,
    pub resize_delta: i32,
    pub new_window_behaviour: WindowContainerBehaviour,
    pub cross_monitor_move_behaviour: MoveBehaviour,
    pub work_area_offset: Option<Rect>,
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    pub mouse_follows_focus: bool,
    pub has_pending_raise_op: bool,
    pub remove_titlebars: bool,
    pub float_identifiers: Vec<IdWithIdentifier>,
    pub manage_identifiers: Vec<IdWithIdentifier>,
    pub layered_whitelist: Vec<IdWithIdentifier>,
    pub tray_and_multi_window_identifiers: Vec<IdWithIdentifier>,
    pub border_overflow_identifiers: Vec<IdWithIdentifier>,
    pub name_change_on_launch_identifiers: Vec<IdWithIdentifier>,
    pub monitor_index_preferences: HashMap<usize, Rect>,
    pub display_index_preferences: HashMap<usize, String>
}