use crate::scrolling::WorkspaceScrolling;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Write;
use std::num::NonZeroUsize;

pub use komorebi_core::WorkspaceLayer;
pub use komorebi_core::WorkspaceWindowLocation;
pub(crate) use komorebi_core::sorted_layout_options_rules;

/// The workspace over the Windows [`Window`], which is `komorebi_core::Workspace` with this
/// platform's methods added by [`WorkspaceExt`] and the scrolling ones of [`WorkspaceScrolling`]
pub type Workspace = komorebi_core::Workspace<WindowsPlatform>;

/// The Windows daemon as the platform of the shared workspace
#[derive(Debug, Clone, PartialEq)]
pub struct WindowsPlatform;

impl komorebi_core::WorkspacePlatform for WindowsPlatform {
    type Window = Window;
    type State = ();

    fn default_padding() -> (Option<i32>, Option<i32>) {
        (
            Option::from(crate::settings::container_padding()),
            Option::from(crate::settings::workspace_padding()),
        )
    }

    fn foreground_window_id() -> Option<PlatformWindowId> {
        crate::window::foreground_window_id()
    }

    fn cursor_position() -> Option<(i32, i32)> {
        crate::window::cursor_position()
    }

    fn should_act_window(
        title: &str,
        exe_name: &str,
        class: &str,
        path: &str,
        rules: &[crate::core::config_generation::MatchingRule],
    ) -> Option<crate::core::config_generation::MatchingRule> {
        should_act_window(
            title,
            exe_name,
            class,
            path,
            rules,
            &REGEX_IDENTIFIERS.lock(),
        )
    }

    fn should_act_window_individual(
        title: &str,
        exe_name: &str,
        class: &str,
        path: &str,
        rule: &crate::core::config_generation::IdWithIdentifier,
    ) -> bool {
        should_act_window_individual(
            title,
            exe_name,
            class,
            path,
            rule,
            &REGEX_IDENTIFIERS.lock(),
        )
    }

    fn primary_container_idx(layout: &Layout) -> eyre::Result<usize> {
        Ok(match layout {
            Layout::Default(_) => 0,
            Layout::Custom(layout) => layout.first_container_idx(
                layout
                    .primary_idx()
                    .ok_or_eyre("this custom layout does not have a primary column")?,
            ),
        })
    }

    fn scrolling_idx_for_direction(
        workspace: &Workspace,
        direction: OperationDirection,
    ) -> Option<usize> {
        workspace.scrolling_idx_for_direction(direction)
    }

    fn scrolling_insertion_idx(workspace: &Workspace) -> Option<usize> {
        workspace.scrolling_insertion_idx()
    }

    fn note_scrolling_departure(workspace: &Workspace, idx: usize) {
        workspace.note_scrolling_departure(idx);
    }
}
use std::sync::atomic::Ordering;

use crate::DATA_DIR;
use crate::HIDDEN_HWNDS;
use crate::INITIAL_CONFIGURATION_LOADED;
use crate::KomorebiTheme;
use crate::NO_TITLEBAR;
use crate::REGEX_IDENTIFIERS;
use crate::REMOVE_TITLEBARS;
use crate::SocketMessage;
use crate::Wallpaper;
use crate::animation::engine::AnimationBatch;
use crate::border_manager;
use crate::container::Container;
use crate::container::ContainerExt;
use crate::core::CustomLayout;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::LayoutDefaultEntry;
use crate::core::OperationDirection;
use crate::core::WindowKind;
use crate::should_act;
use crate::should_act_window;
use crate::should_act_window_individual;
use crate::stackbar_manager;
use crate::stackbar_manager::STACKBAR_TAB_HEIGHT;
use crate::static_config::WorkspaceConfig;
use crate::window::PlatformWindowId;
use crate::window::Window;
use crate::window::WindowDetails;
use crate::windows_api::WindowsApi;
use crate::windows_backend::backend;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_backend::WindowId;
use komorebi_core::CachedLayoutDefault;
use komorebi_core::ContainerWindow;
pub use komorebi_core::WorkspaceGlobals;
use komorebi_themes::Base16ColourPalette;
use komorebi_themes::KomorebiThemeCustom as Custom;
use uds_windows::UnixStream;

pub trait WorkspaceExt {
    fn load_static_config(
        &mut self,
        config: &WorkspaceConfig,
        layout_defaults: Option<&HashMap<DefaultLayout, LayoutDefaultEntry>>,
    ) -> eyre::Result<()>;

    fn apply_wallpaper(&self, hmonitor: isize, monitor_wp: &Option<Wallpaper>) -> eyre::Result<()>;

    fn restore(
        &mut self,
        mouse_follows_focus: bool,
        hmonitor: isize,
        monitor_wp: &Option<Wallpaper>,
    ) -> eyre::Result<()>;

    fn update(&mut self) -> eyre::Result<()>;

    fn hwnd_from_exe(&self, exe: &str) -> Option<isize>;

    fn remove_window(&mut self, window_id: isize) -> eyre::Result<()>;

    fn new_maximized_window(&mut self) -> eyre::Result<()>;

    fn reintegrate_maximized_window(&mut self) -> eyre::Result<()>;

    /// The windows that are on show, which are the ones to slide when the
    /// workspace is switched
    fn windows_on_show(&self) -> Vec<Window>;

    /// What kind of border each of the windows that are on show has
    fn border_kinds_on_show(&self) -> Vec<(isize, WindowKind)>;

    fn visible_window_details(&self) -> Vec<WindowDetails>;
}

impl WorkspaceExt for Workspace {
    fn load_static_config(
        &mut self,
        config: &WorkspaceConfig,
        layout_defaults: Option<&HashMap<DefaultLayout, LayoutDefaultEntry>>,
    ) -> eyre::Result<()> {
        self.name = Option::from(config.name.clone());

        self.container_padding = config.container_padding;

        self.workspace_padding = config.workspace_padding;

        if let Some(layout) = &config.layout {
            self.layout = Layout::Default(*layout);
        }

        #[allow(deprecated)]
        if let Some(pathbuf) = &config.custom_layout {
            let layout = CustomLayout::from_path(pathbuf)?;
            self.layout = Layout::Custom(layout);
        }

        #[allow(deprecated)]
        {
            self.tile = !(config.custom_layout.is_none()
                && config.layout.is_none()
                && config.tile.is_none()
                || config.tile.is_some_and(|tile| !tile));
        }

        let mut all_layout_rules = vec![];
        if let Some(layout_rules) = &config.layout_rules {
            for (count, rule) in layout_rules {
                all_layout_rules.push((*count, Layout::Default(*rule)));
            }

            all_layout_rules.sort_by_key(|(i, _)| *i);
            self.tile = true;
        }

        self.layout_rules = all_layout_rules.clone();

        #[allow(deprecated)]
        if let Some(layout_rules) = &config.custom_layout_rules {
            for (count, pathbuf) in layout_rules {
                let rule = CustomLayout::from_path(pathbuf)?;
                all_layout_rules.push((*count, Layout::Custom(rule)));
            }

            all_layout_rules.sort_by_key(|(i, _)| *i);
            self.tile = true;
            self.layout_rules = all_layout_rules;
        }

        let mut all_work_area_offset_rules = vec![];
        if let Some(work_area_offset_rules) = &config.work_area_offset_rules {
            for (count, rect) in work_area_offset_rules {
                all_work_area_offset_rules.push((*count, *rect));
            }
            all_work_area_offset_rules.sort_by_key(|(i, _)| *i);
            self.work_area_offset_rules = all_work_area_offset_rules;
        }

        self.work_area_offset = config.work_area_offset;

        self.apply_window_based_work_area_offset =
            config.apply_window_based_work_area_offset.unwrap_or(true);

        self.window_container_behaviour = config.window_container_behaviour;

        if let Some(window_container_behaviour_rules) = &config.window_container_behaviour_rules {
            if window_container_behaviour_rules.is_empty() {
                self.window_container_behaviour_rules = None;
            } else {
                let mut all_rules = vec![];
                for (count, behaviour) in window_container_behaviour_rules {
                    all_rules.push((*count, *behaviour));
                }

                all_rules.sort_by_key(|(i, _)| *i);
                self.window_container_behaviour_rules = Some(all_rules);
            }
        } else {
            self.window_container_behaviour_rules = None;
        }

        self.float_override = config.float_override;
        self.layout_flip = config.layout_flip;
        self.floating_layer_behaviour = config.floating_layer_behaviour;
        self.wallpaper = config.wallpaper.clone();

        // Load layout options directly (LayoutOptions is used in both config and runtime)
        self.layout_options = config.layout_options;
        self.initial_window_placement_rules = config.initial_window_placement_rules.clone();

        // Load threshold-based layout options rules, sorted by threshold ascending
        self.layout_options_rules =
            sorted_layout_options_rules(config.layout_options_rules.as_ref());

        tracing::debug!(
            "Workspace '{}' loaded layout_options: {:?}, layout_options_rules: {} entries",
            self.name.as_deref().unwrap_or("unnamed"),
            self.layout_options,
            self.layout_options_rules.len(),
        );

        // Cache per-layout defaults from global layout_defaults, pre-sorting rules
        self.layout_defaults_cache = if let Some(defaults) = layout_defaults {
            defaults
                .iter()
                .map(|(layout, entry)| {
                    (
                        *layout,
                        CachedLayoutDefault {
                            layout_options: entry.layout_options,
                            layout_options_rules: sorted_layout_options_rules(
                                entry.layout_options_rules.as_ref(),
                            ),
                        },
                    )
                })
                .collect()
        } else {
            HashMap::new()
        };

        self.workspace_config = Some(config.clone());

        Ok(())
    }

    fn apply_wallpaper(&self, hmonitor: isize, monitor_wp: &Option<Wallpaper>) -> eyre::Result<()> {
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
                    let komorebi_theme = KomorebiTheme::Custom(Custom {
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
                    });

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

    fn restore(
        &mut self,
        mouse_follows_focus: bool,
        hmonitor: isize,
        monitor_wp: &Option<Wallpaper>,
    ) -> eyre::Result<()> {
        if let Some(container) = &self.monocle_container
            && let Some(window) = container.focused_window()
        {
            let _ = container.restore();
            window.focus(mouse_follows_focus)?;
            return self.apply_wallpaper(hmonitor, monitor_wp);
        }

        let idx = self.focused_container_idx();
        let mut to_focus = None;

        for (i, container) in self.containers_mut().iter_mut().enumerate() {
            if let Some(window) = container.focused_window_mut()
                && idx == i
            {
                to_focus = Option::from(*window);
            }

            let _ = container.restore();
        }

        if let Some(container) = self.focused_container_mut() {
            container.focus_window(container.focused_window_idx());
        }

        for window in self.floating_windows() {
            let _ = window.restore_window();
        }

        // Do this here to make sure that an error doesn't stop the restoration of other windows
        // Maximised windows and floating windows should always be drawn at the top of the Z order
        // when switching to a workspace
        if let Some(window) = to_focus {
            if self.maximized_window.is_none() && matches!(self.layer, WorkspaceLayer::Tiling) {
                window.focus(mouse_follows_focus)?;
            } else if let Some(maximized_window) = self.maximized_window {
                let _ = maximized_window.restore_window();
                maximized_window.focus(mouse_follows_focus)?;
            } else if let Some(floating_window) = self.focused_floating_window() {
                floating_window.focus(mouse_follows_focus)?;
            }
        } else if let Some(maximized_window) = self.maximized_window {
            let _ = maximized_window.restore_window();
            maximized_window.focus(mouse_follows_focus)?;
        } else if let Some(floating_window) = self.focused_floating_window() {
            floating_window.focus(mouse_follows_focus)?;
        }

        self.apply_wallpaper(hmonitor, monitor_wp)
    }

    fn update(&mut self) -> eyre::Result<()> {
        if !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
            return Ok(());
        }

        // every window that moves in this pass is animated together
        let _animations = AnimationBatch::begin();

        // make sure we are never holding on to empty containers
        self.containers_mut()
            .retain(|c| c.is_preselect() || !c.windows().is_empty());

        let container_padding = self
            .container_padding
            .or(self.globals.container_padding)
            .unwrap_or_default();
        let workspace_padding = self
            .workspace_padding
            .or(self.globals.workspace_padding)
            .unwrap_or_default();
        let border_width = self.globals.border_width;
        let border_offset = self.globals.border_offset;
        let work_area = self.globals.work_area;
        let window_based_work_area_offset = self.globals.window_based_work_area_offset;
        let window_based_work_area_offset_limit = self.globals.window_based_work_area_offset_limit;
        let mut rules_work_area_offset = None;

        if !self.work_area_offset_rules.is_empty() {
            let count = if self.monocle_container.is_some() {
                1
            } else {
                self.containers().len()
            };

            for (threshold, work_area_offset_rule) in &self.work_area_offset_rules {
                if count >= *threshold {
                    rules_work_area_offset = Some(*work_area_offset_rule);
                }
            }
        };

        let work_area_offset = rules_work_area_offset
            .or(self.work_area_offset)
            .or(self.globals.work_area_offset);

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
            || self.monocle_container.is_some() && window_based_work_area_offset_limit > 0)
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

        if !self.layout_rules.is_empty() {
            let mut updated_layout = None;

            for (threshold, layout) in &self.layout_rules {
                if self.containers().len() >= *threshold {
                    updated_layout = Option::from(layout.clone());
                }
            }

            if let Some(updated_layout) = updated_layout {
                self.layout = updated_layout;
            }
        }

        if let Some(window_container_behaviour_rules) = &self.window_container_behaviour_rules {
            let mut updated_behaviour = None;
            for (threshold, behaviour) in window_container_behaviour_rules {
                if self.containers().len() >= *threshold {
                    updated_behaviour = Option::from(*behaviour);
                }
            }

            self.window_container_behaviour = updated_behaviour;
        }

        let managed_maximized_window = self.maximized_window.is_some();

        if self.tile {
            if let Some(container) = &mut self.monocle_container {
                if let Some(window) = container.focused_window_mut() {
                    adjusted_work_area.add_padding(container_padding);
                    adjusted_work_area.add_padding(border_offset);
                    adjusted_work_area.add_padding(border_width);
                    window.set_position(&adjusted_work_area, true)?;
                };
            } else if let Some(window) = &mut self.maximized_window {
                window.maximize();
            } else if !self.containers().is_empty() {
                // windows that were not what they were told to be since the last layout, which
                // is something that animations only show once they are done
                let around_window = container_padding + border_offset + border_width;
                // neighbours of a window that is dragged are not yet the size they are being
                // made while it is followed: that is not what their columns are to be
                if self.is_scrolling()
                    && crate::scrolling::animations_enabled()
                    && !crate::live_resize::is_stepping()
                {
                    self.adopt_actual_widths(around_window, adjusted_work_area.right);
                }

                let mut pass = 0;
                loop {
                    let effective_layout_options = self.effective_layout_options();

                    tracing::debug!(
                        "Workspace '{}' update() - effective_layout_options: {:?} (base: {:?}, rules: {})",
                        self.name.as_deref().unwrap_or("unnamed"),
                        effective_layout_options,
                        self.layout_options,
                        self.layout_options_rules.len(),
                    );
                    let mut hidden = vec![];
                    let mut layouts = if self.is_scrolling() {
                        let others = crate::scrolling::other_monitors(
                            &self.globals.work_area,
                            &WindowsApi::monitor_rects(),
                        );

                        self.layout_scrolling(&adjusted_work_area, container_padding)
                            .into_iter()
                            .map(|placement| {
                                let (rect, hide) = crate::scrolling::rest(
                                    placement.rect,
                                    placement.visibility,
                                    &adjusted_work_area,
                                    &others,
                                );
                                hidden.push(hide);
                                rect
                            })
                            .collect()
                    } else {
                        self.layout.as_boxed_arrangement().calculate(
                            &adjusted_work_area,
                            NonZeroUsize::new(self.containers().len()).ok_or_eyre(
                                "there must be at least one container to calculate a workspace layout",
                            )?,
                            Some(container_padding),
                            self.layout_flip,
                            &self.resize_dimensions,
                            self.focused_container_idx(),
                            effective_layout_options,
                            &self.latest_layout,
                        )
                    };

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

                                    if should_remove_titlebars && should_remove_titlebar_for_window
                                    {
                                        window.remove_title_bar()?;
                                    } else if should_remove_titlebar_for_window {
                                        window.add_title_bar()?;
                                    }

                                    // If a window has been unmaximized via toggle-maximize, this block
                                    // will make sure that it is unmaximized via restore_window
                                    if window.is_maximized() && !managed_maximized_window {
                                        let _ =
                                            backend().unminimize(WindowId::from_hwnd(window.hwnd));
                                    }
                                }
                                if hidden.get(i).copied().unwrap_or_default() {
                                    // it is seen on another monitor wherever it rests here
                                    let _ = window.hide_window(());
                                } else {
                                    if crate::scrolling::should_restore_when_placed(
                                        window.hwnd,
                                        container.focused_window().map(|w| w.hwnd),
                                        &HIDDEN_HWNDS.lock(),
                                    ) {
                                        let _ = window.restore_window();
                                    }

                                    backend().set_rect(WindowId::from_hwnd(window.hwnd), layout)?;
                                }
                            }
                        }
                    }

                    self.latest_layout = layouts;

                    // with animations off every window that changed width has been asked what it
                    // is by now, so a window that refused is known, and everything after it is
                    // placed again with room for it. Once is enough: a layout is never made to
                    // chase the sizes that are read back this time round.
                    if pass == 0
                        && self.is_scrolling()
                        && !crate::scrolling::animations_enabled()
                        && self.adopt_actual_widths(around_window, adjusted_work_area.right)
                    {
                        pass += 1;
                        continue;
                    }

                    break;
                }
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
        if self.monocle_container.is_none() {
            self.resize_dimensions.resize(container_count, None);
        }

        Ok(())
    }

    fn hwnd_from_exe(&self, exe: &str) -> Option<isize> {
        for container in self.containers() {
            if let Some(window_id) = container.hwnd_from_exe(exe) {
                return Option::from(window_id);
            }
        }

        if let Some(window) = self.maximized_window
            && let Some(window_exe) = window.exe()
            && exe == window_exe
        {
            return Option::from(window.hwnd);
        }

        if let Some(container) = &self.monocle_container
            && let Some(window_id) = container.hwnd_from_exe(exe)
        {
            return Option::from(window_id);
        }

        for window in self.floating_windows() {
            if let Some(window_exe) = window.exe()
                && exe == window_exe
            {
                return Option::from(window.hwnd);
            }
        }

        None
    }

    fn remove_window(&mut self, window_id: isize) -> eyre::Result<()> {
        border_manager::delete_border(window_id);

        if self.floating_windows().iter().any(|w| w.hwnd == window_id) {
            self.floating_windows_mut().retain(|w| w.hwnd != window_id);
            return Ok(());
        }

        if let Some(container) = &mut self.monocle_container
            && let Some(window_idx) = container
                .windows()
                .iter()
                .position(|window| window.hwnd == window_id)
        {
            container
                .remove_window_by_idx(window_idx)
                .ok_or_eyre("there is no window")?;

            if container.windows().is_empty() {
                self.monocle_container = None;
                self.monocle_container_restore_idx = None;
            }

            for c in self.containers() {
                let _ = c.restore();
            }

            return Ok(());
        }

        if let Some(window) = self.maximized_window
            && window.hwnd == window_id
        {
            window.unmaximize();
            self.maximized_window = None;
            self.maximized_window_restore_idx = None;
            return Ok(());
        }

        let container_idx = self
            .container_idx_for_window(window_id)
            .ok_or_eyre("there is no window")?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_eyre("there is no container")?;

        let window_idx = container
            .windows()
            .iter()
            .position(|window| window.hwnd == window_id)
            .ok_or_eyre("there is no window")?;

        container
            .remove_window_by_idx(window_idx)
            .ok_or_eyre("there is no window")?;

        if container.windows().is_empty() {
            let container_id = container.id.clone();
            self.remove_container_by_idx(container_idx);

            if !self.focus_after_scrolling_removal(&container_id) {
                self.focus_previous_container();
            }
        } else {
            let _ = container.load_focused_window(());
            if let Some(window) = container.focused_window() {
                window.focus(false)?;
            }
        }

        Ok(())
    }

    fn new_maximized_window(&mut self) -> eyre::Result<()> {
        let focused_idx = self.focused_container_idx();

        if matches!(self.layer, WorkspaceLayer::Floating) {
            let floating_window_idx = self.focused_floating_window_idx();
            let floating_window = self.floating_windows_mut().remove(floating_window_idx);
            self.maximized_window = floating_window;
            self.maximized_window_restore_idx = Option::from(focused_idx);
            if let Some(window) = self.maximized_window {
                window.maximize();
            }

            return Ok(());
        }

        let monocle_restore_idx = self.monocle_container_restore_idx;
        if let Some(monocle_container) = &mut self.monocle_container {
            let window = monocle_container
                .remove_focused_window()
                .ok_or_eyre("there is no window")?;

            if monocle_container.windows().is_empty() {
                self.monocle_container = None;
                self.monocle_container_restore_idx = None;
            } else {
                let _ = monocle_container.load_focused_window(());
            }

            self.maximized_window = Option::from(window);
            self.maximized_window_restore_idx = monocle_restore_idx;
            if let Some(window) = self.maximized_window {
                window.maximize();
            }

            return Ok(());
        }

        let container = self
            .focused_container_mut()
            .ok_or_eyre("there is no container")?;

        let window = container
            .remove_focused_window()
            .ok_or_eyre("there is no window")?;

        if container.windows().is_empty() {
            // we shouldn't use remove_container_by_idx here because it doesn't make sense for
            // monocle and maximized toggles which take over the whole screen before being reinserted
            // at the same index to respect locked container indexes
            self.containers_mut().remove(focused_idx);
            if self.resize_dimensions.get(focused_idx).is_some() {
                self.resize_dimensions.remove(focused_idx);
            }
        } else {
            let _ = container.load_focused_window(());
        }

        self.maximized_window = Option::from(window);
        self.maximized_window_restore_idx = Option::from(focused_idx);

        if let Some(window) = self.maximized_window {
            window.maximize();
        }

        self.focus_previous_container();

        Ok(())
    }

    fn reintegrate_maximized_window(&mut self) -> eyre::Result<()> {
        let restore_idx = self
            .maximized_window_restore_idx
            .ok_or_eyre("there is no maximized restore index")?;

        let window = self
            .maximized_window
            .ok_or_eyre("there is no maximized window")?;

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
            .ok_or_eyre("there is no container")?
            .load_focused_window(())?;

        self.maximized_window = None;
        self.maximized_window_restore_idx = None;

        Ok(())
    }

    fn windows_on_show(&self) -> Vec<Window> {
        if let Some(monocle) = &self.monocle_container {
            return monocle.focused_window().copied().into_iter().collect();
        }

        self.visible_windows()
            .into_iter()
            .flatten()
            .copied()
            .collect()
    }

    fn border_kinds_on_show(&self) -> Vec<(isize, WindowKind)> {
        if let Some(monocle) = &self.monocle_container {
            return monocle
                .focused_window()
                .map(|window| (window.hwnd, WindowKind::Monocle))
                .into_iter()
                .collect();
        }

        let mut kinds: Vec<(isize, WindowKind)> = self
            .containers()
            .iter()
            .enumerate()
            .filter_map(|(idx, container)| {
                let kind = if idx != self.focused_container_idx() {
                    if container.locked {
                        WindowKind::UnfocusedLocked
                    } else {
                        WindowKind::Unfocused
                    }
                } else if container.windows().len() > 1 {
                    WindowKind::Stack
                } else {
                    WindowKind::Single
                };

                container.focused_window().map(|window| (window.hwnd, kind))
            })
            .collect();

        kinds.extend(
            self.floating_windows()
                .iter()
                .map(|window| (window.hwnd, WindowKind::Unfocused)),
        );

        kinds
    }

    fn visible_window_details(&self) -> Vec<WindowDetails> {
        let mut vec: Vec<WindowDetails> = vec![];

        if let Some(maximized) = self.maximized_window {
            vec.push(WindowDetails::from(maximized));
        }

        if let Some(monocle) = &self.monocle_container
            && let Some(focused) = monocle.focused_window()
        {
            vec.push(WindowDetails::from(*focused));
        }

        for container in self.containers() {
            if let Some(focused) = container.focused_window() {
                vec.push(WindowDetails::from(*focused));
            }
        }

        for window in self.floating_windows() {
            {
                vec.push(WindowDetails::from(*window));
            }
        }

        vec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Window;
    use crate::container::Container;
    use std::collections::HashMap;

    #[test]
    fn test_locked_containers_with_new_window() {
        let mut ws = Workspace::default();

        let mut state = HashMap::new();

        // add 4 containers
        for i in 0..4 {
            let mut container = Container::default();
            if i == 3 {
                container.locked = true; // set index 3 locked
            }
            state.insert(i, container.id.to_string());
            ws.add_container_to_back(container);
        }
        assert_eq!(ws.containers().len(), 4);

        // focus container at index 2
        ws.focus_container(2);

        // simulate a new window being launched on this workspace
        let _ = ws.new_container_for_window(Window::from(123));

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
        let _ = ws.new_container_for_window(Window::from(234));
        assert_eq!(
            ws.containers()[3].id.to_string(),
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
                container.locked = true;
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
                container.locked = true;
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
        let _ = container.add_window(window, ());
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
                container.locked = true;
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
        assert!(
            !workspace
                .containers()
                .contains(&container_to_remove.unwrap())
        );
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
        assert!(
            !workspace
                .containers()
                .contains(&container_to_remove.unwrap())
        );
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
        assert!(
            !workspace
                .containers()
                .contains(&container_to_remove.unwrap())
        );
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
        let _ = workspace.new_container_for_window(Window::from(2));

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

        // Should be focused on window 1 and window_id should be 1
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

        // Should contain window_id 0 since this is the first window in the container
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
        workspace.maximized_window = Some(Window { hwnd: 200 });

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
