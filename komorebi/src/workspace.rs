use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io::Write;
use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;

use crate::border_manager;
use crate::container::Container;
use crate::core::Axis;
use crate::core::CustomLayout;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::default_layout::LayoutOptions;
use crate::lockable_sequence::LockableSequence;
use crate::ring::Ring;
use crate::should_act;
use crate::stackbar_manager;
use crate::stackbar_manager::STACKBAR_TAB_HEIGHT;
use crate::static_config::WorkspaceConfig;
use crate::window::Window;
use crate::window::WindowDetails;
use crate::windows_api::WindowsApi;
use crate::FloatingLayerBehaviour;
use crate::KomorebiTheme;
use crate::SocketMessage;
use crate::Wallpaper;
use crate::WindowContainerBehaviour;
use crate::DATA_DIR;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::INITIAL_CONFIGURATION_LOADED;
use crate::NO_TITLEBAR;
use crate::REGEX_IDENTIFIERS;
use crate::REMOVE_TITLEBARS;
use color_eyre::eyre::anyhow;
use color_eyre::Result;
use getset::CopyGetters;
use getset::Getters;
use getset::MutGetters;
use getset::Setters;
use komorebi_themes::Base16ColourPalette;
use serde::Deserialize;
use serde::Serialize;
use uds_windows::UnixStream;

#[allow(clippy::struct_field_names)]
#[derive(
    Debug, Clone, Serialize, Deserialize, Getters, CopyGetters, MutGetters, Setters, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Workspace {
    #[getset(get = "pub", set = "pub")]
    pub name: Option<String>,
    pub containers: Ring<Container>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub monocle_container: Option<Container>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[getset(get_copy = "pub", set = "pub")]
    pub monocle_container_restore_idx: Option<usize>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub maximized_window: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[getset(get_copy = "pub", set = "pub")]
    pub maximized_window_restore_idx: Option<usize>,
    pub floating_windows: Ring<Window>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub layout: Layout,
    #[getset(get_copy = "pub", set = "pub")]
    pub layout_options: Option<LayoutOptions>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub layout_rules: Vec<(usize, Layout)>,
    #[getset(get_copy = "pub", set = "pub")]
    pub layout_flip: Option<Axis>,
    #[getset(get_copy = "pub", set = "pub")]
    pub workspace_padding: Option<i32>,
    #[getset(get_copy = "pub", set = "pub")]
    pub container_padding: Option<i32>,
    #[getset(get = "pub", set = "pub")]
    pub latest_layout: Vec<Rect>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub resize_dimensions: Vec<Option<Rect>>,
    #[getset(get = "pub", set = "pub")]
    pub tile: bool,
    #[getset(get_copy = "pub", set = "pub")]
    pub apply_window_based_work_area_offset: bool,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub window_container_behaviour: Option<WindowContainerBehaviour>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub window_container_behaviour_rules: Option<Vec<(usize, WindowContainerBehaviour)>>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub float_override: Option<bool>,
    #[serde(skip)]
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub globals: WorkspaceGlobals,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub layer: WorkspaceLayer,
    #[getset(get_copy = "pub", get_mut = "pub", set = "pub")]
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    pub wallpaper: Option<Wallpaper>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[getset(get = "pub", set = "pub")]
    pub workspace_config: Option<WorkspaceConfig>,
}

#[derive(Debug, Default, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum WorkspaceLayer {
    #[default]
    Tiling,
    Floating,
}

impl Display for WorkspaceLayer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceLayer::Tiling => write!(f, "Tiling"),
            WorkspaceLayer::Floating => write!(f, "Floating"),
        }
    }
}

impl_ring_elements!(Workspace, Container);
impl_ring_elements!(Workspace, Window, "floating_window");

impl Default for Workspace {
    fn default() -> Self {
        Self {
            name: None,
            containers: Ring::default(),
            monocle_container: None,
            maximized_window: None,
            maximized_window_restore_idx: None,
            monocle_container_restore_idx: None,
            floating_windows: Ring::default(),
            layout: Layout::Default(DefaultLayout::BSP),
            layout_options: None,
            layout_rules: vec![],
            layout_flip: None,
            workspace_padding: Option::from(DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst)),
            container_padding: Option::from(DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst)),
            latest_layout: vec![],
            resize_dimensions: vec![],
            tile: true,
            apply_window_based_work_area_offset: true,
            window_container_behaviour: None,
            window_container_behaviour_rules: None,
            float_override: None,
            layer: Default::default(),
            floating_layer_behaviour: Default::default(),
            globals: Default::default(),
            workspace_config: None,
            wallpaper: None,
        }
    }
}

#[derive(Debug)]
pub enum WorkspaceWindowLocation {
    Monocle(usize), // window_idx
    Maximized,
    Container(usize, usize), // container_idx, window_idx
    Floating(usize),         // idx in floating_windows
}

#[derive(
    Debug,
    Default,
    Copy,
    Clone,
    Serialize,
    Deserialize,
    Getters,
    CopyGetters,
    MutGetters,
    Setters,
    PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Settings setup either by the parent monitor or by the `WindowManager`
pub struct WorkspaceGlobals {
    pub container_padding: Option<i32>,
    pub workspace_padding: Option<i32>,
    pub border_width: i32,
    pub border_offset: i32,
    pub work_area: Rect,
    pub work_area_offset: Option<Rect>,
    pub window_based_work_area_offset: Option<Rect>,
    pub window_based_work_area_offset_limit: isize,
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
}

impl Workspace {
    pub fn load_static_config(&mut self, config: &WorkspaceConfig) -> Result<()> {
        self.name = Option::from(config.name.clone());

        self.set_container_padding(config.container_padding);

        self.set_workspace_padding(config.workspace_padding);

        if let Some(layout) = &config.layout {
            self.layout = Layout::Default(*layout);
            self.tile = true;
        }

        if let Some(pathbuf) = &config.custom_layout {
            let layout = CustomLayout::from_path(pathbuf)?;
            self.layout = Layout::Custom(layout);
            self.tile = true;
        }

        if config.custom_layout.is_none() && config.layout.is_none() {
            self.tile = false;
        }

        let mut all_layout_rules = vec![];
        if let Some(layout_rules) = &config.layout_rules {
            for (count, rule) in layout_rules {
                all_layout_rules.push((*count, Layout::Default(*rule)));
            }

            all_layout_rules.sort_by_key(|(i, _)| *i);
            self.tile = true;
        }

        self.set_layout_rules(all_layout_rules.clone());

        if let Some(layout_rules) = &config.custom_layout_rules {
            for (count, pathbuf) in layout_rules {
                let rule = CustomLayout::from_path(pathbuf)?;
                all_layout_rules.push((*count, Layout::Custom(rule)));
            }

            all_layout_rules.sort_by_key(|(i, _)| *i);
            self.tile = true;
            self.set_layout_rules(all_layout_rules);
        }

        self.set_apply_window_based_work_area_offset(
            config.apply_window_based_work_area_offset.unwrap_or(true),
        );

        self.set_window_container_behaviour(config.window_container_behaviour);

        if let Some(window_container_behaviour_rules) = &config.window_container_behaviour_rules {
            if window_container_behaviour_rules.is_empty() {
                self.set_window_container_behaviour_rules(None);
            } else {
                let mut all_rules = vec![];
                for (count, behaviour) in window_container_behaviour_rules {
                    all_rules.push((*count, *behaviour));
                }

                all_rules.sort_by_key(|(i, _)| *i);
                self.set_window_container_behaviour_rules(Some(all_rules));
            }
        } else {
            self.set_window_container_behaviour_rules(None);
        }

        self.set_float_override(config.float_override);
        self.set_layout_flip(config.layout_flip);
        self.set_floating_layer_behaviour(config.floating_layer_behaviour);
        self.set_wallpaper(config.wallpaper.clone());
        self.set_layout_options(config.layout_options);

        self.set_workspace_config(Some(config.clone()));

        Ok(())
    }

    pub fn hide(&mut self, omit: Option<isize>) {
        for window in self.floating_windows_mut().iter_mut().rev() {
            let mut should_hide = omit.is_none();

            if !should_hide {
                if let Some(omit) = omit {
                    if omit != window.hwnd {
                        should_hide = true
                    }
                }
            }

            if should_hide {
                window.hide();
            }
        }

        for container in self.containers_mut() {
            container.hide(omit)
        }

        if let Some(window) = self.maximized_window() {
            window.hide();
        }

        if let Some(container) = self.monocle_container_mut() {
            container.hide(omit)
        }
    }

    pub fn apply_wallpaper(&self, hmonitor: isize, monitor_wp: &Option<Wallpaper>) -> Result<()> {
        if let Some(wallpaper) = self.wallpaper.as_ref().or(monitor_wp.as_ref()) {
            if let Err(error) = WindowsApi::set_wallpaper(&wallpaper.path, hmonitor) {
                tracing::error!("failed to set wallpaper: {error}");
            }

            if wallpaper.generate_theme.unwrap_or(true) {
                let variant = wallpaper
                    .theme_options
                    .as_ref()
                    .and_then(|t| t.theme_variant)
                    .unwrap_or_default();

                let cached_palette = DATA_DIR.join(format!(
                    "{}.base16.{variant}.json",
                    wallpaper
                        .path
                        .file_name()
                        .unwrap_or(OsStr::new("tmp"))
                        .to_string_lossy()
                ));

                let mut base16_palette = None;

                if cached_palette.is_file() {
                    tracing::info!(
                        "colour palette for wallpaper {} found in cache",
                        cached_palette.display()
                    );

                    // this code is VERY slow on debug builds - should only be a one-time issue when loading
                    // an uncached wallpaper
                    if let Ok(palette) = serde_json::from_str::<Base16ColourPalette>(
                        &std::fs::read_to_string(&cached_palette)?,
                    ) {
                        base16_palette = Some(palette);
                    }
                };

                if base16_palette.is_none() {
                    base16_palette =
                        komorebi_themes::generate_base16_palette(&wallpaper.path, variant).ok();

                    std::fs::write(
                        &cached_palette,
                        serde_json::to_string_pretty(&base16_palette)?,
                    )?;

                    tracing::info!(
                        "colour palette for wallpaper {} cached",
                        cached_palette.display()
                    );
                }

                if let Some(palette) = base16_palette {
                    let komorebi_theme = KomorebiTheme::Custom {
                        colours: Box::new(palette),
                        single_border: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.single_border),
                        stack_border: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.stack_border),
                        monocle_border: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.monocle_border),
                        floating_border: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.floating_border),
                        unfocused_border: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.unfocused_border),
                        unfocused_locked_border: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.unfocused_locked_border),
                        stackbar_focused_text: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.stackbar_focused_text),
                        stackbar_unfocused_text: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.stackbar_unfocused_text),
                        stackbar_background: wallpaper
                            .theme_options
                            .as_ref()
                            .and_then(|o| o.stackbar_background),
                        bar_accent: wallpaper.theme_options.as_ref().and_then(|o| o.bar_accent),
                    };

                    let bytes = SocketMessage::Theme(Box::new(komorebi_theme)).as_bytes()?;

                    let socket = DATA_DIR.join("komorebi.sock");
                    match UnixStream::connect(socket) {
                        Ok(mut stream) => {
                            if let Err(error) = stream.write_all(&bytes) {
                                tracing::error!("failed to send theme update message: {error}")
                            }
                        }
                        Err(error) => {
                            tracing::error!("{error}")
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn restore(
        &mut self,
        mouse_follows_focus: bool,
        hmonitor: isize,
        monitor_wp: &Option<Wallpaper>,
    ) -> Result<()> {
        if let Some(container) = self.monocle_container() {
            if let Some(window) = container.focused_window() {
                container.restore();
                window.focus(mouse_follows_focus)?;
                return self.apply_wallpaper(hmonitor, monitor_wp);
            }
        }

        let idx = self.focused_container_idx();
        let mut to_focus = None;

        for (i, container) in self.containers_mut().iter_mut().enumerate() {
            if let Some(window) = container.focused_window_mut() {
                if idx == i {
                    to_focus = Option::from(*window);
                }
            }

            container.restore();
        }

        if let Some(container) = self.focused_container_mut() {
            container.focus_window(container.focused_window_idx());
        }

        for window in self.floating_windows() {
            window.restore();
        }

        // Do this here to make sure that an error doesn't stop the restoration of other windows
        // Maximised windows and floating windows should always be drawn at the top of the Z order
        // when switching to a workspace
        if let Some(window) = to_focus {
            if self.maximized_window().is_none() && matches!(self.layer, WorkspaceLayer::Tiling) {
                window.focus(mouse_follows_focus)?;
            } else if let Some(maximized_window) = self.maximized_window() {
                maximized_window.restore();
                maximized_window.focus(mouse_follows_focus)?;
            } else if let Some(floating_window) = self.focused_floating_window() {
                floating_window.focus(mouse_follows_focus)?;
            }
        } else if let Some(maximized_window) = self.maximized_window() {
            maximized_window.restore();
            maximized_window.focus(mouse_follows_focus)?;
        } else if let Some(floating_window) = self.focused_floating_window() {
            floating_window.focus(mouse_follows_focus)?;
        }

        self.apply_wallpaper(hmonitor, monitor_wp)
    }

    pub fn update(&mut self) -> Result<()> {
        if !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
            return Ok(());
        }

        // make sure we are never holding on to empty containers
        self.containers_mut().retain(|c| !c.windows().is_empty());

        let container_padding = self
            .container_padding()
            .or(self.globals().container_padding)
            .unwrap_or_default();
        let workspace_padding = self
            .workspace_padding()
            .or(self.globals().workspace_padding)
            .unwrap_or_default();
        let border_width = self.globals().border_width;
        let border_offset = self.globals().border_offset;
        let work_area = self.globals().work_area;
        let work_area_offset = self.globals().work_area_offset;
        let window_based_work_area_offset = self.globals().window_based_work_area_offset;
        let window_based_work_area_offset_limit =
            self.globals().window_based_work_area_offset_limit;

        let mut adjusted_work_area = work_area_offset.map_or_else(
            || work_area,
            |offset| {
                let mut with_offset = work_area;
                with_offset.left += offset.left;
                with_offset.top += offset.top;
                with_offset.right -= offset.right;
                with_offset.bottom -= offset.bottom;

                with_offset
            },
        );

        if (self.containers().len() <= window_based_work_area_offset_limit as usize
            || self.monocle_container().is_some() && window_based_work_area_offset_limit > 0)
            && self.apply_window_based_work_area_offset
        {
            adjusted_work_area = window_based_work_area_offset.map_or_else(
                || adjusted_work_area,
                |offset| {
                    let mut with_offset = adjusted_work_area;
                    with_offset.left += offset.left;
                    with_offset.top += offset.top;
                    with_offset.right -= offset.right;
                    with_offset.bottom -= offset.bottom;

                    with_offset
                },
            );
        }

        adjusted_work_area.add_padding(workspace_padding);

        self.enforce_resize_constraints();

        if !self.layout_rules().is_empty() {
            let mut updated_layout = None;

            for (threshold, layout) in self.layout_rules() {
                if self.containers().len() >= *threshold {
                    updated_layout = Option::from(layout.clone());
                }
            }

            if let Some(updated_layout) = updated_layout {
                self.set_layout(updated_layout);
            }
        }

        if let Some(window_container_behaviour_rules) = self.window_container_behaviour_rules() {
            let mut updated_behaviour = None;
            for (threshold, behaviour) in window_container_behaviour_rules {
                if self.containers().len() >= *threshold {
                    updated_behaviour = Option::from(*behaviour);
                }
            }

            self.set_window_container_behaviour(updated_behaviour);
        }

        let managed_maximized_window = self.maximized_window().is_some();

        if *self.tile() {
            if let Some(container) = self.monocle_container_mut() {
                if let Some(window) = container.focused_window_mut() {
                    adjusted_work_area.add_padding(container_padding);
                    adjusted_work_area.add_padding(border_offset);
                    adjusted_work_area.add_padding(border_width);
                    window.set_position(&adjusted_work_area, true)?;
                };
            } else if let Some(window) = self.maximized_window_mut() {
                window.maximize();
            } else if !self.containers().is_empty() {
                let mut layouts = self.layout().as_boxed_arrangement().calculate(
                    &adjusted_work_area,
                    NonZeroUsize::new(self.containers().len()).ok_or_else(|| {
                        anyhow!(
                            "there must be at least one container to calculate a workspace layout"
                        )
                    })?,
                    Some(container_padding),
                    self.layout_flip(),
                    self.resize_dimensions(),
                    self.focused_container_idx(),
                    self.layout_options(),
                    self.latest_layout(),
                );

                let should_remove_titlebars = REMOVE_TITLEBARS.load(Ordering::SeqCst);
                let no_titlebar = NO_TITLEBAR.lock().clone();
                let regex_identifiers = REGEX_IDENTIFIERS.lock().clone();

                let containers = self.containers_mut();

                for (i, container) in containers.iter_mut().enumerate() {
                    let window_count = container.windows().len();

                    if let Some(layout) = layouts.get_mut(i) {
                        layout.add_padding(border_offset);
                        layout.add_padding(border_width);

                        if stackbar_manager::should_have_stackbar(window_count) {
                            let tab_height = STACKBAR_TAB_HEIGHT.load(Ordering::SeqCst);
                            let total_height = tab_height + container_padding;

                            layout.top += total_height;
                            layout.bottom -= total_height;
                        }

                        for window in container.windows() {
                            if container
                                .focused_window()
                                .is_some_and(|w| w.hwnd == window.hwnd)
                            {
                                let should_remove_titlebar_for_window = should_act(
                                    &window.title().unwrap_or_default(),
                                    &window.exe().unwrap_or_default(),
                                    &window.class().unwrap_or_default(),
                                    &window.path().unwrap_or_default(),
                                    &no_titlebar,
                                    &regex_identifiers,
                                )
                                .is_some();

                                if should_remove_titlebars && should_remove_titlebar_for_window {
                                    window.remove_title_bar()?;
                                } else if should_remove_titlebar_for_window {
                                    window.add_title_bar()?;
                                }

                                // If a window has been unmaximized via toggle-maximize, this block
                                // will make sure that it is unmaximized via restore_window
                                if window.is_maximized() && !managed_maximized_window {
                                    WindowsApi::restore_window(window.hwnd);
                                }
                            }
                            window.set_position(layout, false)?;
                        }
                    }
                }

                self.set_latest_layout(layouts);
            }
        }

        // Always make sure that the length of the resize dimensions vec is the same as the
        // number of layouts / containers. This should never actually truncate as the remove_window
        // function takes care of cleaning up resize dimensions when destroying empty containers
        let container_count = self.containers().len();

        // since monocle is a toggle, we never want to truncate the resize dimensions since it will
        // almost always be toggled off and the container will be reintegrated into layout
        //
        // without this check, if there are exactly two containers, when one is toggled to monocle
        // the resize dimensions will be truncated to len == 1, and when it is reintegrated, if it
        // had a resize adjustment before, that will have been lost
        if self.monocle_container().is_none() {
            self.resize_dimensions_mut().resize(container_count, None);
        }

        Ok(())
    }

    pub fn container_for_window(&self, hwnd: isize) -> Option<&Container> {
        self.containers().get(self.container_idx_for_window(hwnd)?)
    }

    /// If there is a container which holds the window with `hwnd` it will focus that container.
    /// This function will only emit a focus on the window if it isn't the focused window of that
    /// container already.
    pub fn focus_container_by_window(&mut self, hwnd: isize) -> Result<()> {
        let container_idx = self
            .container_idx_for_window(hwnd)
            .ok_or_else(|| anyhow!("there is no container/window"))?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window_idx = container
            .idx_for_window(hwnd)
            .ok_or_else(|| anyhow!("there is no window"))?;

        let mut should_load = false;

        if container.focused_window_idx() != window_idx {
            should_load = true
        }

        container.focus_window(window_idx);

        if should_load {
            container.load_focused_window();
        }

        self.focus_container(container_idx);

        Ok(())
    }

    pub fn container_idx_from_current_point(&self) -> Option<usize> {
        let mut idx = None;

        let point = WindowsApi::cursor_pos().ok()?;

        for (i, _container) in self.containers().iter().enumerate() {
            if let Some(rect) = self.latest_layout().get(i) {
                if rect.contains_point((point.x, point.y)) {
                    idx = Option::from(i);
                }
            }
        }

        idx
    }

    pub fn hwnd_from_exe(&self, exe: &str) -> Option<isize> {
        for container in self.containers() {
            if let Some(hwnd) = container.hwnd_from_exe(exe) {
                return Option::from(hwnd);
            }
        }

        if let Some(window) = self.maximized_window() {
            if let Ok(window_exe) = window.exe() {
                if exe == window_exe {
                    return Option::from(window.hwnd);
                }
            }
        }

        if let Some(container) = self.monocle_container() {
            if let Some(hwnd) = container.hwnd_from_exe(exe) {
                return Option::from(hwnd);
            }
        }

        for window in self.floating_windows() {
            if let Ok(window_exe) = window.exe() {
                if exe == window_exe {
                    return Option::from(window.hwnd);
                }
            }
        }

        None
    }

    pub fn location_from_exe(&self, exe: &str) -> Option<WorkspaceWindowLocation> {
        for (container_idx, container) in self.containers().iter().enumerate() {
            if let Some(window_idx) = container.idx_from_exe(exe) {
                return Some(WorkspaceWindowLocation::Container(
                    container_idx,
                    window_idx,
                ));
            }
        }

        if let Some(window) = self.maximized_window() {
            if let Ok(window_exe) = window.exe() {
                if exe == window_exe {
                    return Some(WorkspaceWindowLocation::Maximized);
                }
            }
        }

        if let Some(container) = self.monocle_container() {
            if let Some(window_idx) = container.idx_from_exe(exe) {
                return Some(WorkspaceWindowLocation::Monocle(window_idx));
            }
        }

        for (window_idx, window) in self.floating_windows().iter().enumerate() {
            if let Ok(window_exe) = window.exe() {
                if exe == window_exe {
                    return Some(WorkspaceWindowLocation::Floating(window_idx));
                }
            }
        }

        None
    }

    pub fn contains_managed_window(&self, hwnd: isize) -> bool {
        for container in self.containers() {
            if container.contains_window(hwnd) {
                return true;
            }
        }

        if let Some(window) = self.maximized_window() {
            if hwnd == window.hwnd {
                return true;
            }
        }

        if let Some(container) = self.monocle_container() {
            if container.contains_window(hwnd) {
                return true;
            }
        }

        false
    }

    pub fn is_focused_window_monocle_or_maximized(&self) -> Result<bool> {
        let hwnd = WindowsApi::foreground_window()?;
        if let Some(window) = self.maximized_window() {
            if hwnd == window.hwnd {
                return Ok(true);
            }
        }

        if let Some(container) = self.monocle_container() {
            if container.contains_window(hwnd) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn is_empty(&self) -> bool {
        self.containers().is_empty()
            && self.maximized_window().is_none()
            && self.monocle_container().is_none()
            && self.floating_windows().is_empty()
    }

    pub fn contains_window(&self, hwnd: isize) -> bool {
        for container in self.containers() {
            if container.contains_window(hwnd) {
                return true;
            }
        }

        if let Some(window) = self.maximized_window() {
            if hwnd == window.hwnd {
                return true;
            }
        }

        if let Some(container) = self.monocle_container() {
            if container.contains_window(hwnd) {
                return true;
            }
        }

        for window in self.floating_windows() {
            if hwnd == window.hwnd {
                return true;
            }
        }

        false
    }

    pub fn promote_container(&mut self) -> Result<()> {
        let resize = self.resize_dimensions_mut().remove(0);
        let container = self
            .remove_focused_container()
            .ok_or_else(|| anyhow!("there is no container"))?;

        let primary_idx = match self.layout() {
            Layout::Default(_) => 0,
            Layout::Custom(layout) => layout.first_container_idx(
                layout
                    .primary_idx()
                    .ok_or_else(|| anyhow!("this custom layout does not have a primary column"))?,
            ),
        };

        let insertion_idx = self.insert_container_at_idx(primary_idx, container);
        self.resize_dimensions_mut()[insertion_idx] = resize;
        self.focus_container(primary_idx);

        Ok(())
    }

    pub fn add_container_to_back(&mut self, container: Container) {
        self.containers_mut().push_back(container);
        self.focus_last_container();
    }

    pub fn add_container_to_front(&mut self, container: Container) {
        self.containers_mut().push_front(container);
        self.focus_first_container();
    }

    // this fn respects locked container indexes - we should use it for pretty much everything
    // except monocle and maximize toggles
    pub fn insert_container_at_idx(&mut self, idx: usize, container: Container) -> usize {
        let insertion_idx = self
            .containers_mut()
            .insert_respecting_locks(idx, container);

        if insertion_idx > self.resize_dimensions().len() {
            self.resize_dimensions_mut().push(None);
        } else {
            self.resize_dimensions_mut().insert(insertion_idx, None);
        }

        self.focus_container(insertion_idx);

        insertion_idx
    }

    // this fn respects locked container indexes - we should use it for pretty much everything
    // except monocle and maximize toggles
    pub fn remove_container_by_idx(&mut self, idx: usize) -> Option<Container> {
        let container = self.containers_mut().remove_respecting_locks(idx);

        if idx < self.resize_dimensions().len() {
            self.resize_dimensions_mut().remove(idx);
        }

        container
    }

    pub fn container_idx_for_window(&self, hwnd: isize) -> Option<usize> {
        let mut idx = None;
        for (i, x) in self.containers().iter().enumerate() {
            if x.contains_window(hwnd) {
                idx = Option::from(i);
            }
        }

        idx
    }

    pub fn remove_window(&mut self, hwnd: isize) -> Result<()> {
        border_manager::delete_border(hwnd);

        if self.floating_windows().iter().any(|w| w.hwnd == hwnd) {
            self.floating_windows_mut().retain(|w| w.hwnd != hwnd);
            return Ok(());
        }

        if let Some(container) = self.monocle_container_mut() {
            if let Some(window_idx) = container
                .windows()
                .iter()
                .position(|window| window.hwnd == hwnd)
            {
                container
                    .remove_window_by_idx(window_idx)
                    .ok_or_else(|| anyhow!("there is no window"))?;

                if container.windows().is_empty() {
                    self.set_monocle_container(None);
                    self.set_monocle_container_restore_idx(None);
                }

                for c in self.containers() {
                    c.restore();
                }

                return Ok(());
            }
        }

        if let Some(window) = self.maximized_window() {
            if window.hwnd == hwnd {
                window.unmaximize();
                self.set_maximized_window(None);
                self.set_maximized_window_restore_idx(None);
                return Ok(());
            }
        }

        let container_idx = self
            .container_idx_for_window(hwnd)
            .ok_or_else(|| anyhow!("there is no window"))?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window_idx = container
            .windows()
            .iter()
            .position(|window| window.hwnd == hwnd)
            .ok_or_else(|| anyhow!("there is no window"))?;

        container
            .remove_window_by_idx(window_idx)
            .ok_or_else(|| anyhow!("there is no window"))?;

        if container.windows().is_empty() {
            self.remove_container_by_idx(container_idx);
            self.focus_previous_container();
        } else {
            container.load_focused_window();
            if let Some(window) = container.focused_window() {
                window.focus(false)?;
            }
        }

        Ok(())
    }

    pub fn remove_focused_container(&mut self) -> Option<Container> {
        let focused_idx = self.focused_container_idx();
        let container = self.remove_container_by_idx(focused_idx);
        self.focus_previous_container();

        container
    }

    pub fn remove_container(&mut self, idx: usize) -> Option<Container> {
        let container = self.remove_container_by_idx(idx);
        self.focus_previous_container();

        container
    }

    pub fn new_idx_for_direction(&self, direction: OperationDirection) -> Option<usize> {
        let len = NonZeroUsize::new(self.containers().len())?;

        direction.destination(
            self.layout().as_boxed_direction().as_ref(),
            self.layout_flip(),
            self.focused_container_idx(),
            len,
        )
    }

    pub fn new_idx_for_cycle_direction(&self, direction: CycleDirection) -> Option<usize> {
        Option::from(direction.next_idx(
            self.focused_container_idx(),
            NonZeroUsize::new(self.containers().len())?,
        ))
    }

    // this is what we use for stacking
    pub fn move_window_to_container(&mut self, target_container_idx: usize) -> Result<()> {
        let focused_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window = container
            .remove_focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;

        // This is a little messy
        let adjusted_target_container_index = if container.windows().is_empty() {
            self.remove_container_by_idx(focused_idx);

            if focused_idx < target_container_idx {
                target_container_idx.saturating_sub(1)
            } else {
                target_container_idx
            }
        } else {
            container.load_focused_window();
            target_container_idx
        };

        let target_container = self
            .containers_mut()
            .get_mut(adjusted_target_container_index)
            .ok_or_else(|| anyhow!("there is no container"))?;

        target_container.add_window(window);

        self.focus_container(adjusted_target_container_index);
        self.focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?
            .load_focused_window();

        Ok(())
    }

    pub fn new_container_for_focused_window(&mut self) -> Result<()> {
        let focused_container_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window = container
            .remove_focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;

        if container.windows().is_empty() {
            self.remove_container_by_idx(focused_container_idx);
        } else {
            container.load_focused_window();
        }

        self.new_container_for_window(window);

        let mut container = Container::default();
        container.add_window(window);
        Ok(())
    }

    pub fn new_container_for_floating_window(&mut self) -> Result<()> {
        let focused_idx = self.focused_container_idx();
        let window = self
            .remove_focused_floating_window()
            .ok_or_else(|| anyhow!("there is no floating window"))?;

        let mut container = Container::default();
        container.add_window(window);

        self.insert_container_at_idx(focused_idx, container);

        Ok(())
    }

    pub fn new_container_for_window(&mut self, window: Window) {
        let next_idx = if self.containers().is_empty() {
            0
        } else {
            self.focused_container_idx() + 1
        };

        let mut container = Container::default();
        container.add_window(window);

        self.insert_container_at_idx(next_idx, container);
    }

    pub fn new_floating_window(&mut self) -> Result<()> {
        let window = if let Some(maximized_window) = self.maximized_window() {
            let window = *maximized_window;
            self.set_maximized_window(None);
            self.set_maximized_window_restore_idx(None);
            window
        } else if let Some(monocle_container) = self.monocle_container_mut() {
            let window = monocle_container
                .remove_focused_window()
                .ok_or_else(|| anyhow!("there is no window"))?;

            if monocle_container.windows().is_empty() {
                self.set_monocle_container(None);
                self.set_monocle_container_restore_idx(None);
            } else {
                monocle_container.load_focused_window();
            }

            window
        } else {
            let focused_idx = self.focused_container_idx();

            let container = self
                .focused_container_mut()
                .ok_or_else(|| anyhow!("there is no container"))?;

            let window = container
                .remove_focused_window()
                .ok_or_else(|| anyhow!("there is no window"))?;

            if container.windows().is_empty() {
                self.remove_container_by_idx(focused_idx);

                if focused_idx == self.containers().len() {
                    self.focus_container(focused_idx.saturating_sub(1));
                }
            } else {
                container.load_focused_window();
            }

            window
        };

        self.floating_windows_mut().push_back(window);

        Ok(())
    }

    fn enforce_resize_constraints(&mut self) {
        match self.layout {
            Layout::Default(DefaultLayout::BSP) => self.enforce_resize_constraints_for_bsp(),
            Layout::Default(DefaultLayout::Columns) => self.enforce_resize_for_columns(),
            Layout::Default(DefaultLayout::Rows) => self.enforce_resize_for_rows(),
            Layout::Default(DefaultLayout::VerticalStack) => {
                self.enforce_resize_for_vertical_stack();
            }
            Layout::Default(DefaultLayout::RightMainVerticalStack) => {
                self.enforce_resize_for_right_vertical_stack();
            }
            Layout::Default(DefaultLayout::HorizontalStack) => {
                self.enforce_resize_for_horizontal_stack();
            }
            Layout::Default(DefaultLayout::UltrawideVerticalStack) => {
                self.enforce_resize_for_ultrawide();
            }
            Layout::Default(DefaultLayout::Scrolling) => {
                self.enforce_resize_for_scrolling();
            }
            _ => self.enforce_no_resize(),
        }
    }

    fn enforce_resize_constraints_for_bsp(&mut self) {
        for (i, rect) in self.resize_dimensions_mut().iter_mut().enumerate() {
            if let Some(rect) = rect {
                // Even containers can't be resized to the bottom
                if i % 2 == 0 {
                    rect.bottom = 0;
                    // Odd containers can't be resized to the right
                } else {
                    rect.right = 0;
                }
            }
        }

        // The first container can never be resized to the left or the top
        if let Some(Some(first)) = self.resize_dimensions_mut().first_mut() {
            first.top = 0;
            first.left = 0;
        }

        // The last container can never be resized to the bottom or the right
        if let Some(Some(last)) = self.resize_dimensions_mut().last_mut() {
            last.bottom = 0;
            last.right = 0;
        }
    }

    fn enforce_resize_for_columns(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                let len = resize_dimensions.len();
                for (i, rect) in resize_dimensions.iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.top = 0;
                        rect.bottom = 0;

                        if i == 0 {
                            rect.left = 0;
                        }
                        if i == len - 1 {
                            rect.right = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_rows(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                let len = resize_dimensions.len();
                for (i, rect) in resize_dimensions.iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.left = 0;
                        rect.right = 0;

                        if i == 0 {
                            rect.top = 0;
                        }
                        if i == len - 1 {
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_vertical_stack(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            // Single window can not be resized at all
            0 | 1 => self.enforce_no_resize(),
            _ => {
                // Zero is actually on the left
                if let Some(mut left) = resize_dimensions[0] {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }

                // Handle stack on the right
                let stack_size = resize_dimensions[1..].len();
                for (i, rect) in resize_dimensions[1..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        // No containers can resize to the right
                        rect.right = 0;

                        // First container in stack cant resize up
                        if i == 0 {
                            rect.top = 0;
                        } else if i == stack_size - 1 {
                            // Last cant be resized to the bottom
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_right_vertical_stack(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            // Single window can not be resized at all
            0 | 1 => self.enforce_no_resize(),
            _ => {
                // Zero is actually on the right
                if let Some(mut left) = resize_dimensions[1] {
                    left.top = 0;
                    left.bottom = 0;
                    left.right = 0;
                }

                // Handle stack on the right
                let stack_size = resize_dimensions[1..].len();
                for (i, rect) in resize_dimensions[1..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        // No containers can resize to the left
                        rect.left = 0;

                        // First container in stack cant resize up
                        if i == 0 {
                            rect.top = 0;
                        } else if i == stack_size - 1 {
                            // Last cant be resized to the bottom
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_horizontal_stack(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                if let Some(mut left) = resize_dimensions[0] {
                    left.top = 0;
                    left.left = 0;
                    left.right = 0;
                }

                let stack_size = resize_dimensions[1..].len();
                for (i, rect) in resize_dimensions[1..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.bottom = 0;

                        if i == 0 {
                            rect.left = 0;
                        }
                        if i == stack_size - 1 {
                            rect.right = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_ultrawide(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            // Single window can not be resized at all
            0 | 1 => self.enforce_no_resize(),
            // Two windows can only be resized in the middle
            2 => {
                // Zero is actually on the right
                if let Some(mut right) = resize_dimensions[0] {
                    right.top = 0;
                    right.bottom = 0;
                    right.right = 0;
                }

                // One is on the left
                if let Some(mut left) = resize_dimensions[1] {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }
            }
            // Three or more windows means 0 is in center, 1 is at the left, 2.. are a vertical
            // stack on the right
            _ => {
                // Central can be resized left or right
                if let Some(mut right) = resize_dimensions[0] {
                    right.top = 0;
                    right.bottom = 0;
                }

                // Left one can only be resized to the right
                if let Some(mut left) = resize_dimensions[1] {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }

                // Handle stack on the right
                let stack_size = resize_dimensions[2..].len();
                for (i, rect) in resize_dimensions[2..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        // No containers can resize to the right
                        rect.right = 0;

                        // First container in stack cant resize up
                        if i == 0 {
                            rect.top = 0;
                        } else if i == stack_size - 1 {
                            // Last cant be resized to the bottom
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_scrolling(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                let len = resize_dimensions.len();

                for (i, rect) in resize_dimensions.iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.top = 0;
                        rect.bottom = 0;

                        if i == 0 {
                            rect.left = 0;
                        } else if i == len - 1 {
                            rect.right = 0;
                        }
                    }
                }
            }
        }
    }
    fn enforce_no_resize(&mut self) {
        for rect in self.resize_dimensions_mut().iter_mut().flatten() {
            rect.left = 0;
            rect.right = 0;
            rect.top = 0;
            rect.bottom = 0;
        }
    }

    pub fn new_monocle_container(&mut self) -> Result<()> {
        let focused_idx = self.focused_container_idx();

        // we shouldn't use remove_container_by_idx here because it doesn't make sense for
        // monocle and maximized toggles which take over the whole screen before being reinserted
        // at the same index to respect locked container indexes
        let container = self
            .containers_mut()
            .remove(focused_idx)
            .ok_or_else(|| anyhow!("there is no container"))?;

        // We don't remove any resize adjustments for a monocle, because when this container is
        // inevitably reintegrated, it would be weird if it doesn't go back to the dimensions
        // it had before

        self.set_monocle_container(Option::from(container));
        self.set_monocle_container_restore_idx(Option::from(focused_idx));
        self.focus_previous_container();

        self.monocle_container_mut()
            .as_mut()
            .ok_or_else(|| anyhow!("there is no monocle container"))?
            .load_focused_window();

        Ok(())
    }

    pub fn reintegrate_monocle_container(&mut self) -> Result<()> {
        let restore_idx = self
            .monocle_container_restore_idx()
            .ok_or_else(|| anyhow!("there is no monocle restore index"))?;

        let container = self
            .monocle_container_mut()
            .as_ref()
            .ok_or_else(|| anyhow!("there is no monocle container"))?;

        let container = container.clone();
        if restore_idx >= self.containers().len() {
            self.containers_mut()
                .resize(restore_idx, Container::default());
        }

        // we shouldn't use insert_container_at_index here because it doesn't make sense for
        // monocle and maximized toggles which take over the whole screen before being reinserted
        // at the same index to respect locked container indexes
        self.containers_mut().insert(restore_idx, container);
        self.focus_container(restore_idx);
        self.focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?
            .load_focused_window();

        self.set_monocle_container(None);
        self.set_monocle_container_restore_idx(None);

        Ok(())
    }

    pub fn new_maximized_window(&mut self) -> Result<()> {
        let focused_idx = self.focused_container_idx();

        if matches!(self.layer, WorkspaceLayer::Floating) {
            let floating_window_idx = self.focused_floating_window_idx();
            let floating_window = self.floating_windows_mut().remove(floating_window_idx);
            self.set_maximized_window(floating_window);
            self.set_maximized_window_restore_idx(Option::from(focused_idx));
            if let Some(window) = self.maximized_window() {
                window.maximize();
            }

            return Ok(());
        }

        let monocle_restore_idx = self.monocle_container_restore_idx();
        if let Some(monocle_container) = self.monocle_container_mut() {
            let window = monocle_container
                .remove_focused_window()
                .ok_or_else(|| anyhow!("there is no window"))?;

            if monocle_container.windows().is_empty() {
                self.set_monocle_container(None);
                self.set_monocle_container_restore_idx(None);
            } else {
                monocle_container.load_focused_window();
            }

            self.set_maximized_window(Option::from(window));
            self.set_maximized_window_restore_idx(monocle_restore_idx);
            if let Some(window) = self.maximized_window() {
                window.maximize();
            }

            return Ok(());
        }

        let container = self
            .focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?;

        let window = container
            .remove_focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;

        if container.windows().is_empty() {
            // we shouldn't use remove_container_by_idx here because it doesn't make sense for
            // monocle and maximized toggles which take over the whole screen before being reinserted
            // at the same index to respect locked container indexes
            self.containers_mut().remove(focused_idx);
            if self.resize_dimensions().get(focused_idx).is_some() {
                self.resize_dimensions_mut().remove(focused_idx);
            }
        } else {
            container.load_focused_window();
        }

        self.set_maximized_window(Option::from(window));
        self.set_maximized_window_restore_idx(Option::from(focused_idx));

        if let Some(window) = self.maximized_window() {
            window.maximize();
        }

        self.focus_previous_container();

        Ok(())
    }

    pub fn reintegrate_maximized_window(&mut self) -> Result<()> {
        let restore_idx = self
            .maximized_window_restore_idx()
            .ok_or_else(|| anyhow!("there is no monocle restore index"))?;

        let window = self
            .maximized_window()
            .as_ref()
            .ok_or_else(|| anyhow!("there is no monocle container"))?;

        let window = *window;
        if !self.containers().is_empty() && restore_idx > self.containers().len().saturating_sub(1)
        {
            self.containers_mut()
                .resize(restore_idx, Container::default());
        }

        let mut container = Container::default();
        container.windows_mut().push_back(window);

        // we shouldn't use insert_container_at_index here because it doesn't make sense for
        // monocle and maximized toggles which take over the whole screen before being reinserted
        // at the same index to respect locked container indexes
        self.containers_mut().insert(restore_idx, container);
        self.focus_container(restore_idx);

        self.focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))?
            .load_focused_window();

        self.set_maximized_window(None);
        self.set_maximized_window_restore_idx(None);

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container(&mut self, idx: usize) {
        tracing::info!("focusing container");

        self.containers.focus(idx);
    }

    pub fn swap_containers(&mut self, i: usize, j: usize) {
        self.containers.swap(i, j);
        self.focus_container(j);
    }

    pub fn remove_focused_floating_window(&mut self) -> Option<Window> {
        let hwnd = WindowsApi::foreground_window().ok()?;

        let mut idx = None;
        for (i, window) in self.floating_windows().iter().enumerate() {
            if hwnd == window.hwnd {
                idx = Option::from(i);
            }
        }

        match idx {
            None => None,
            Some(idx) => {
                if self.floating_windows().get(idx).is_some() {
                    self.floating_windows_mut().remove(idx)
                } else {
                    None
                }
            }
        }
    }

    pub fn visible_windows(&self) -> Vec<Option<&Window>> {
        let mut vec = vec![];

        vec.push(self.maximized_window().as_ref());

        if let Some(monocle) = self.monocle_container() {
            vec.push(monocle.focused_window());
        }

        for container in self.containers() {
            vec.push(container.focused_window());
        }

        for window in self.floating_windows() {
            vec.push(Some(window));
        }

        vec
    }

    pub fn visible_window_details(&self) -> Vec<WindowDetails> {
        let mut vec: Vec<WindowDetails> = vec![];

        if let Some(maximized) = self.maximized_window() {
            if let Ok(details) = (*maximized).try_into() {
                vec.push(details);
            }
        }

        if let Some(monocle) = self.monocle_container() {
            if let Some(focused) = monocle.focused_window() {
                if let Ok(details) = (*focused).try_into() {
                    vec.push(details);
                }
            }
        }

        for container in self.containers() {
            if let Some(focused) = container.focused_window() {
                if let Ok(details) = (*focused).try_into() {
                    vec.push(details);
                }
            }
        }

        for window in self.floating_windows() {
            if let Ok(details) = (*window).try_into() {
                vec.push(details);
            }
        }

        vec
    }

    pub fn focus_previous_container(&mut self) {
        let focused_idx = self.focused_container_idx();
        self.focus_container(focused_idx.saturating_sub(1));
    }

    fn focus_last_container(&mut self) {
        self.focus_container(self.containers().len().saturating_sub(1));
    }

    fn focus_first_container(&mut self) {
        self.focus_container(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::Container;
    use crate::Window;
    use std::collections::HashMap;

    #[test]
    fn test_locked_containers_with_new_window() {
        let mut ws = Workspace::default();

        let mut state = HashMap::new();

        // add 4 containers
        for i in 0..4 {
            let mut container = Container::default();
            if i == 3 {
                container.set_locked(true); // set index 3 locked
            }
            state.insert(i, container.id().to_string());
            ws.add_container_to_back(container);
        }
        assert_eq!(ws.containers().len(), 4);

        // focus container at index 2
        ws.focus_container(2);

        // simulate a new window being launched on this workspace
        ws.new_container_for_window(Window::from(123));

        // new length should be 5, with the focus on the new window at index 4
        assert_eq!(ws.containers().len(), 5);
        assert_eq!(ws.focused_container_idx(), 4);
        assert_eq!(
            ws.focused_container()
                .unwrap()
                .focused_window()
                .unwrap()
                .hwnd,
            123
        );

        // when inserting a new container at index 0, index 3's container should not change
        ws.focus_container(0);
        ws.new_container_for_window(Window::from(234));
        assert_eq!(
            ws.containers()[3].id().to_string(),
            state.get(&3).unwrap().to_string()
        );
    }

    #[test]
    fn test_locked_containers_remove_window() {
        let mut ws = Workspace::default();

        // add 4 containers
        for i in 0..4 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            if i == 1 {
                container.set_locked(true);
            }
            ws.add_container_to_back(container);
        }
        assert_eq!(ws.containers().len(), 4);

        ws.remove_window(0).unwrap();
        assert_eq!(ws.containers()[0].focused_window().unwrap().hwnd, 2);
        // index 1 should still be the same
        assert_eq!(ws.containers()[1].focused_window().unwrap().hwnd, 1);
        assert_eq!(ws.containers()[2].focused_window().unwrap().hwnd, 3);
    }

    #[test]
    fn test_locked_containers_toggle_float() {
        let mut ws = Workspace::default();

        // add 4 containers
        for i in 0..4 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            if i == 1 {
                container.set_locked(true);
            }
            ws.add_container_to_back(container);
        }
        assert_eq!(ws.containers().len(), 4);

        // set index 0 focused
        ws.focus_container(0);

        // float index 0
        ws.new_floating_window().unwrap();

        assert_eq!(ws.containers()[0].focused_window().unwrap().hwnd, 2);
        // index 1 should still be the same
        assert_eq!(ws.containers()[1].focused_window().unwrap().hwnd, 1);
        assert_eq!(ws.containers()[2].focused_window().unwrap().hwnd, 3);

        // unfloat - have to do this semi-manually becuase of calls to WindowsApi in
        // new_container_for_floating_window which usually handles unfloating
        let window = ws.floating_windows_mut().pop_back().unwrap();
        let mut container = Container::default();
        container.add_window(window);
        ws.insert_container_at_idx(ws.focused_container_idx(), container);

        // all indexes should be at their original position
        for i in 0..4 {
            assert_eq!(
                ws.containers()[i].focused_window().unwrap().hwnd,
                i as isize
            );
        }
    }

    #[test]
    fn test_locked_containers_stack() {
        let mut ws = Workspace::default();

        // add 6 containers
        for i in 0..6 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            if i == 4 {
                container.set_locked(true);
            }
            ws.add_container_to_back(container);
        }
        assert_eq!(ws.containers().len(), 6);

        // set index 3 focused
        ws.focus_container(3);

        // stack index 3 on top of index 2
        ws.move_window_to_container(2).unwrap();

        assert_eq!(ws.containers()[0].focused_window().unwrap().hwnd, 0);
        assert_eq!(ws.containers()[1].focused_window().unwrap().hwnd, 1);
        assert_eq!(ws.containers()[2].windows().len(), 2);
        assert_eq!(ws.containers()[3].focused_window().unwrap().hwnd, 5);
        // index 4 should still be the same
        assert_eq!(ws.containers()[4].focused_window().unwrap().hwnd, 4);

        // unstack
        ws.new_container_for_focused_window().unwrap();

        // all indexes should be at their original position
        for i in 0..6 {
            assert_eq!(
                ws.containers()[i].focused_window().unwrap().hwnd,
                i as isize
            )
        }
    }

    #[test]
    fn test_contains_window() {
        // Create default workspace
        let mut workspace = Workspace::default();

        // Add a window to the container
        let mut container = Container::default();
        container.windows_mut().push_back(Window::from(0));

        // Add container
        workspace.add_container_to_back(container);

        // Should be true
        assert!(workspace.contains_window(0));

        // Should be false
        assert!(!workspace.is_empty())
    }

    #[test]
    fn test_add_container_to_back() {
        let mut workspace = Workspace::default();

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }
        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Get focused container. Should be the index of the last container added
        let container = workspace.focused_container_mut().unwrap();

        // Should be focused on the container with 1 window
        assert_eq!(container.windows().len(), 1);
    }

    #[test]
    fn test_add_container_to_front() {
        let mut workspace = Workspace::default();

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_front(container);
        }

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_front(container);
        }
        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Get focused container. Should be the index of the last container added
        let container = workspace.focused_container_mut().unwrap();

        // Should be focused on the container with 3 windows
        assert_eq!(container.windows().len(), 3);
    }

    #[test]
    fn test_remove_non_existent_window() {
        let mut workspace = Workspace::default();

        {
            // Add a container with one window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }

        // Attempt to remove a non-existent window
        let result = workspace.remove_window(2);

        // Should return an error
        assert!(
            result.is_err(),
            "Expected an error when removing a non-existent window"
        );

        // Get focused container. Should be the index of the last container added
        let container = workspace.focused_container_mut().unwrap();

        // Should still have 1 window
        assert_eq!(container.windows().len(), 1);
    }

    #[test]
    fn test_remove_focused_container() {
        let mut workspace = Workspace::default();

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }
        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Should be focused on the container at index 1
        assert_eq!(workspace.focused_container_idx(), 1);

        // Store the container at index 1 before removal
        let container_to_remove = workspace.containers().get(1).cloned();
        workspace.remove_focused_container();

        // Should only have 1 container
        assert_eq!(workspace.containers().len(), 1);

        // Should be focused on the container at index 0
        assert_eq!(workspace.focused_container_idx(), 0);

        // Ensure the container at index 1 before removal is no longer present
        assert!(container_to_remove.is_some());
        assert!(!workspace
            .containers()
            .contains(&container_to_remove.unwrap()));
    }

    #[test]
    fn test_insert_container_at_idx() {
        let mut workspace = Workspace::default();

        for i in 0..4 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 4 containers
        assert_eq!(workspace.containers().len(), 4);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 3);

        // Insert a container at index 4
        workspace.insert_container_at_idx(4, Container::default());

        // Should have 5 containers
        assert_eq!(workspace.containers().len(), 5);

        // Should be focused on the newly inserted container
        assert_eq!(workspace.focused_container_idx(), 4);
    }

    #[test]
    fn test_remove_container_by_idx() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Store the container at index 1 before removal
        let container_to_remove = workspace.containers().get(1).cloned();

        // Remove the container at index 1
        workspace.remove_container_by_idx(1);

        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Ensure the container at index 1 before removal is no longer present
        assert!(container_to_remove.is_some());
        assert!(!workspace
            .containers()
            .contains(&container_to_remove.unwrap()));
    }

    #[test]
    fn test_remove_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Store the container at index 2 before removal
        let container_to_remove = workspace.containers().get(2).cloned();

        // Remove the container at index 2
        workspace.remove_container(2);

        // Should be focused on the previous container which is index 1
        assert_eq!(workspace.focused_container_idx(), 1);

        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Ensure the container at index 1 before removal is no longer present
        assert!(container_to_remove.is_some());
        assert!(!workspace
            .containers()
            .contains(&container_to_remove.unwrap()));
    }

    #[test]
    fn test_focus_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Focus on container 1
        workspace.focus_container(1);
        assert_eq!(workspace.focused_container_idx(), 1);

        // Focus on container 0
        workspace.focus_container(0);
        assert_eq!(workspace.focused_container_idx(), 0);

        // Focus on container 2
        workspace.focus_container(2);
        assert_eq!(workspace.focused_container_idx(), 2);
    }

    #[test]
    fn test_focus_previous_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Focus on the previous container
        workspace.focus_previous_container();

        // Should be focused on container 1
        assert_eq!(workspace.focused_container_idx(), 1);
    }

    #[test]
    fn test_focus_last_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Change focus to the first container for the test
        workspace.focus_container(0);
        assert_eq!(workspace.focused_container_idx(), 0);

        // Focus on the last container
        workspace.focus_last_container();

        // Should be focused on container 1
        assert_eq!(workspace.focused_container_idx(), 2);
    }

    #[test]
    fn test_focus_first_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Focus on the first container
        workspace.focus_first_container();

        // Should be focused on container 1
        assert_eq!(workspace.focused_container_idx(), 0);
    }

    #[test]
    fn test_swap_containers() {
        let mut workspace = Workspace::default();

        {
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }

        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        {
            // Should be focused on container 1
            assert_eq!(workspace.focused_container_idx(), 1);

            // Should have 1 window
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
        }

        // Swap containers 0 and 1
        workspace.swap_containers(0, 1);

        {
            // Should be focused on container 0
            assert_eq!(workspace.focused_container_idx(), 1);

            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 3);
        }
    }

    #[test]
    fn test_new_container_for_window() {
        let mut workspace = Workspace::default();

        {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }

        // Add new window to container
        workspace.new_container_for_window(Window::from(2));

        // Container 0 should have 1 window
        let container = workspace.focused_container_mut().unwrap();
        assert_eq!(container.windows().len(), 1);

        // Should return true that window 2 exists
        assert!(workspace.contains_window(2));
    }

    #[test]
    fn test_move_window_to_container() {
        let mut workspace = Workspace::default();

        {
            // Container with 0 windows
            let container = Container::default();
            workspace.add_container_to_back(container);
        }

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        // Move A Window from container 1 to container 0
        workspace.move_window_to_container(0).unwrap();

        // Focus on container 0
        workspace.focus_container(0);

        // Container 0 should have 1 window
        let container = workspace.focused_container_mut().unwrap();
        assert_eq!(container.windows().len(), 1);
    }

    #[test]
    fn test_move_window_to_non_existent_container() {
        let mut workspace = Workspace::default();

        // Add a container with one window
        let mut container = Container::default();
        container.windows_mut().push_back(Window::from(1));
        workspace.add_container_to_back(container);

        // Try to move window to a non-existent container
        let result = workspace.move_window_to_container(8);

        // Should return an error
        assert!(
            result.is_err(),
            "Expected an error when moving a window to a non-existent container"
        );
    }

    #[test]
    fn test_remove_window() {
        let mut workspace = Workspace::default();

        {
            // Container with 1 window
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        // Remove window 1
        workspace.remove_window(1).ok();

        // Should have 2 windows
        let container = workspace.focused_container_mut().unwrap();
        assert_eq!(container.windows().len(), 2);

        // Check that window 1 is removed
        assert!(!workspace.contains_window(1));
    }

    #[test]
    fn test_new_container_for_focused_window() {
        let mut workspace = Workspace::default();

        {
            // Container with 1 window
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        // Add focused window to new container
        workspace.new_container_for_focused_window().ok();

        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        {
            // Inspect new container. Should contain 1 window. Window name should be 0
            workspace.focus_container(1);
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
            assert!(workspace.contains_window(0));
        }
    }

    #[test]
    fn test_focus_container_by_window() {
        let mut workspace = Workspace::default();

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(4));
            workspace.add_container_to_back(container);
        }

        // Focus container by window
        workspace.focus_container_by_window(1).unwrap();

        // Should be focused on workspace 0
        assert_eq!(workspace.focused_container_idx(), 0);

        // Should be focused on window 1 and hwnd should be 1
        let focused_container = workspace.focused_container_mut().unwrap();
        assert_eq!(
            focused_container.focused_window(),
            Some(&Window { hwnd: 1 })
        );
        assert_eq!(focused_container.focused_window_idx(), 1);
    }

    #[test]
    fn test_contains_managed_window() {
        let mut workspace = Workspace::default();

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(4));
            workspace.add_container_to_back(container);
        }

        // Should return true, window is in container 1
        assert!(workspace.contains_managed_window(4));

        // Should return true, all the windows are in container 0
        for i in 0..3 {
            assert!(workspace.contains_managed_window(i));
        }

        // Should return false since window was never added
        assert!(!workspace.contains_managed_window(5));
    }

    #[test]
    fn test_new_floating_window() {
        let mut workspace = Workspace::default();

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        // Add window to floating_windows
        workspace.new_floating_window().ok();

        // Should have 1 floating window
        assert_eq!(workspace.floating_windows().len(), 1);

        // Should have only 2 windows now
        let container = workspace.focused_container_mut().unwrap();
        assert_eq!(container.windows().len(), 2);

        // Should contain hwnd 0 since this is the first window in the container
        let floating_windows = workspace.floating_windows_mut();
        assert!(floating_windows.contains(&Window { hwnd: 0 }));
    }

    #[test]
    fn test_visible_windows() {
        let mut workspace = Workspace::default();

        {
            // Create and add a default Container with 2 windows
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(100));
            container.windows_mut().push_back(Window::from(200));
            workspace.add_container_to_back(container);
        }

        {
            // visible_windows should return None and 100
            let visible_windows = workspace.visible_windows();
            assert_eq!(visible_windows.len(), 2);
            assert!(visible_windows[0].is_none());
            assert_eq!(visible_windows[1].unwrap().hwnd, 100);
        }

        {
            // Create and add a default Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(300));
            workspace.add_container_to_back(container);
        }

        {
            // visible_windows should return None, 100, and 300
            let visible_windows = workspace.visible_windows();
            assert_eq!(visible_windows.len(), 3);
            assert!(visible_windows[0].is_none());
            assert_eq!(visible_windows[1].unwrap().hwnd, 100);
            assert_eq!(visible_windows[2].unwrap().hwnd, 300);
        }

        // Maximize window 200
        workspace.set_maximized_window(Some(Window { hwnd: 200 }));

        {
            // visible_windows should return 200, 100, and 300
            let visible_windows = workspace.visible_windows();
            assert_eq!(visible_windows.len(), 3);
            assert_eq!(visible_windows[0].unwrap().hwnd, 200);
            assert_eq!(visible_windows[1].unwrap().hwnd, 100);
            assert_eq!(visible_windows[2].unwrap().hwnd, 300);
        }
    }
}
