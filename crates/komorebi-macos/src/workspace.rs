//! [`Workspace`], a ring of containers tiled by a layout.
//!
//! The workspace is `komorebi_core::Workspace`, shared with the other komorebi daemons, over the
//! macOS [`Window`] and [`MacosPlatform`]. What is macOS's own (laying the windows out through
//! Accessibility and the animator, restoring them, the wallpaper, loading a workspace from
//! `komorebi.json`, reaping the windows of an application) is [`WorkspaceExt`], and the scrolling
//! layout's methods are `crate::scrolling::WorkspaceScrolling`.

use crate::DATA_DIR;
use crate::REGEX_IDENTIFIERS;
use crate::container::MacosHideSpot;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::LayoutDefaultEntry;
use crate::core::LayoutOptions;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::ScrollingLayoutOptions;
use crate::core::SocketMessage;
use crate::core::WindowHidingPosition;
use crate::core_graphics::CoreGraphicsApi;
use crate::macos_api::MacosApi;
use crate::scrolling::WorkspaceScrolling;
use crate::skylight::CGSMainConnectionID;
use crate::skylight::SLSDisableUpdate;
use crate::skylight::SLSReenableUpdate;
use crate::static_config::Wallpaper;
use crate::static_config::WorkspaceConfig;
use crate::window::Window;
use crate::window::WindowDetails;
use crate::window::should_act;
use crate::window::should_act_individual;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_themes::Base16ColourPalette;
use komorebi_themes::KomorebiTheme;
use komorebi_themes::KomorebiThemeCustom;
use objc2_core_foundation::CGRect;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Write;
use std::num::NonZeroUsize;
use std::os::unix::net::UnixStream;

pub use komorebi_core::CachedLayoutDefault;
pub use komorebi_core::WorkspaceGlobals;
pub use komorebi_core::WorkspaceLayer;
pub use komorebi_core::WorkspaceWindowLocation;
pub(crate) use komorebi_core::sorted_layout_options_rules;

/// The workspace over the macOS [`Window`], which is `komorebi_core::Workspace` with this
/// platform's methods added by [`WorkspaceExt`] and the scrolling ones of [`WorkspaceScrolling`]
pub type Workspace = komorebi_core::Workspace<MacosPlatform>;

/// What only macOS keeps in a workspace
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MacosWorkspaceState {
    /// The corner of the monitor that the workspace is on, which its windows are hidden in; the
    /// monitor sets it with the rest of the workspace's globals
    pub window_hiding_position: WindowHidingPosition,
}

/// komorebi for macOS as the platform of the shared workspace
#[derive(Debug, Clone, PartialEq)]
pub struct MacosPlatform;

impl komorebi_core::WorkspacePlatform for MacosPlatform {
    type Window = Window;
    type State = MacosWorkspaceState;

    fn hide_spot(workspace: &Workspace) -> MacosHideSpot {
        MacosHideSpot {
            position: workspace.state.window_hiding_position,
            screen: None,
        }
    }

    fn hide_spot_on_display(workspace: &Workspace) -> MacosHideSpot {
        MacosHideSpot {
            position: workspace.state.window_hiding_position,
            screen: CoreGraphicsApi::display_bounds_for_window_rect(CGRect::from(
                workspace.globals.work_area,
            )),
        }
    }

    fn foreground_window_id() -> Option<u32> {
        MacosApi::foreground_window_id()
    }

    fn cursor_position() -> Option<(i32, i32)> {
        let point = MacosApi::cursor_pos();
        Some((point.x as i32, point.y as i32))
    }

    fn should_act_window(
        title: &str,
        exe_name: &str,
        class: &str,
        path: &str,
        rules: &[crate::core::config_generation::MatchingRule],
    ) -> Option<crate::core::config_generation::MatchingRule> {
        should_act(
            title,
            exe_name,
            &[class],
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
        should_act_individual(
            title,
            exe_name,
            &[class],
            path,
            rule,
            &REGEX_IDENTIFIERS.lock(),
        )
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

    fn before_floating_removed(workspace: &mut Workspace) {
        workspace.focus_previous_floating_window();
    }
}

/// What a macOS workspace does that the shared one does not
pub trait WorkspaceExt {
    fn load_static_config(
        &mut self,
        config: &WorkspaceConfig,
        layout_defaults: Option<&HashMap<DefaultLayout, LayoutDefaultEntry>>,
    ) -> eyre::Result<()>;

    fn prune_duplicate_windows(&mut self) -> eyre::Result<()>;

    fn reap_invalid_windows_for_application(
        &mut self,
        process_id: i32,
        valid_window_ids: &[u32],
    ) -> eyre::Result<usize>;

    fn remove_window(&mut self, window_id: u32) -> eyre::Result<Window>;

    fn restore(
        &mut self,
        mouse_follows_focus: bool,
        monitor_id: u32,
        monitor_wp: &Option<Wallpaper>,
    ) -> eyre::Result<()>;

    /// Put the windows of the workspace back where they were and focus the one that has focus,
    /// which is all of [`Workspace::restore`] that is windows being moved
    fn restore_windows(&mut self, mouse_follows_focus: bool) -> eyre::Result<()>;

    fn visible_window_details(&self) -> Vec<WindowDetails>;

    /// Set how many columns the Scrolling layout shows at once, keeping every other layout option.
    fn set_scrolling_columns(&mut self, columns: usize);

    /// Ask the layout for where every container goes, before the result is clamped to the work area.
    fn calculate_layouts(
        &mut self,
        work_area: &Rect,
        container_padding: Option<i32>,
    ) -> eyre::Result<Vec<Rect>>;

    fn update(&mut self) -> eyre::Result<()>;

    fn apply_wallpaper(&self, monitor_id: u32, monitor_wp: &Option<Wallpaper>) -> eyre::Result<()>;
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

        self.tile = !(/* config.custom_layout.is_none() && */config.layout.is_none() && config.tile.is_none()
            || config.tile.is_some_and(|tile| !tile));

        let mut all_layout_rules = vec![];
        if let Some(layout_rules) = &config.layout_rules {
            for (count, rule) in layout_rules {
                all_layout_rules.push((*count, Layout::Default(*rule)));
            }

            all_layout_rules.sort_by_key(|(i, _)| *i);
            self.tile = true;
        }

        self.layout_rules = all_layout_rules.clone();

        self.work_area_offset = config.work_area_offset;

        let mut all_work_area_offset_rules = vec![];
        if let Some(work_area_offset_rules) = &config.work_area_offset_rules {
            for (count, rect) in work_area_offset_rules {
                all_work_area_offset_rules.push((*count, *rect));
            }

            all_work_area_offset_rules.sort_by_key(|(i, _)| *i);
        }

        self.work_area_offset_rules = all_work_area_offset_rules;

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

    fn prune_duplicate_windows(&mut self) -> eyre::Result<()> {
        let mut tracker = HashMap::new();
        let mut to_prune_from_containers = vec![];
        let mut to_prune_from_floating = vec![];

        for container in self.containers() {
            for window in container.windows() {
                let entry = tracker.entry(window.id).or_insert(0);
                *entry += 1;
            }
        }

        if let Some(container) = &self.monocle_container {
            for window in container.windows() {
                let entry = tracker.entry(window.id).or_insert(0);
                *entry += 1;
            }
        }

        for (window_id, count) in &tracker {
            if *count > 1 {
                to_prune_from_containers.push(*window_id);
            }
        }

        for id in to_prune_from_containers {
            self.remove_window(id)?;
            tracker.remove(&id);
        }

        for window in self.floating_windows() {
            let entry = tracker.entry(window.id).or_insert(0);
            *entry += 1;
        }

        for (window_id, count) in &tracker {
            if *count > 1 {
                to_prune_from_floating.push(*window_id);
            }
        }

        for id in to_prune_from_floating {
            self.remove_window(id)?;
            tracker.remove(&id);
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn reap_invalid_windows_for_application(
        &mut self,
        process_id: i32,
        valid_window_ids: &[u32],
    ) -> eyre::Result<usize> {
        let mut invalid_window_ids = vec![];
        let mut invalid_floating_window_ids = vec![];
        for container in self.containers() {
            if let Some(focused_window) = container.focused_window()
                && focused_window.application.process_id == process_id
            {
                if !valid_window_ids.contains(&focused_window.id) {
                    if let Ok(rect) = focused_window.window_rect() {
                        tracing::debug!(
                            "reaping window {} ({}): rect = {:?}",
                            focused_window.id,
                            focused_window.title().unwrap_or_default(),
                            rect
                        );
                    }
                    invalid_window_ids.push(focused_window.id);
                }

                // if accessibility API calls fail on AXError::InvalidUIElement
                // let's try and nuke these early
                if !focused_window.alive() {
                    invalid_window_ids.push(focused_window.id);
                }
            }
        }

        if let Some(container) = &self.monocle_container
            && let Some(focused_window) = container.focused_window()
        {
            if focused_window.application.process_id == process_id
                && !valid_window_ids.contains(&focused_window.id)
            {
                if let Ok(rect) = focused_window.window_rect() {
                    tracing::debug!(
                        "reaping monocle window {} ({}): rect = {:?}",
                        focused_window.id,
                        focused_window.title().unwrap_or_default(),
                        rect
                    );
                }
                invalid_window_ids.push(focused_window.id);
            }

            // if accessibility API calls fail on AXError::InvalidUIElement
            // let's try and nuke these early
            if !focused_window.alive() {
                invalid_window_ids.push(focused_window.id);
            }
        }

        for window in self.floating_windows() {
            if window.application.process_id == process_id {
                if !valid_window_ids.contains(&window.id) {
                    if let Ok(rect) = window.window_rect() {
                        tracing::debug!(
                            "reaping floating window {} ({}): rect = {:?}",
                            window.id,
                            window.title().unwrap_or_default(),
                            rect
                        );
                    }
                    invalid_floating_window_ids.push(window.id);
                }

                // if accessibility API calls fail on AXError::InvalidUIElement
                // let's try and nuke these early
                if !window.alive() {
                    invalid_floating_window_ids.push(window.id);
                }
            }
        }

        for window_id in &invalid_window_ids {
            // Ignore errors - window may have already been removed by the reaper thread
            let _ = self.remove_window(*window_id);
            crate::ax_registry::forget(*window_id);
        }

        for window_id in &invalid_floating_window_ids {
            self.remove_floating_window(*window_id);
            crate::ax_registry::forget(*window_id);
        }

        tracing::debug!(
            "reaping invalid windows {invalid_window_ids:?} {invalid_floating_window_ids:?}"
        );

        Ok(invalid_window_ids.len() + invalid_floating_window_ids.len())
    }

    fn remove_window(&mut self, window_id: u32) -> eyre::Result<Window> {
        if let Some(container) = self.monocle_container.as_mut()
            && let Some(window_idx) = container
                .windows()
                .iter()
                .position(|window| window.id == window_id)
        {
            let window = container
                .remove_window_by_idx(window_idx)
                .ok_or_eyre("there is no window")?;

            if container.windows().is_empty() {
                self.monocle_container = None;
                self.monocle_container_restore_idx = None;
            }

            for c in self.containers_mut() {
                c.restore()?;
            }

            return Ok(window);
        }

        let container_idx = self
            .container_idx_for_window(window_id)
            .ok_or_eyre("there is no window")?;

        let spot = self.hide_spot();

        // a strip of columns has to know what there is before it is told what goes
        self.sync_scrolling_containers();

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_eyre("there is no container")?;

        let window_idx = container
            .windows()
            .iter()
            .position(|window| window.id == window_id)
            .ok_or_eyre("there is no window")?;

        let window = container
            .remove_window_by_idx(window_idx)
            .ok_or_eyre("there is no window")?;

        if container.windows().is_empty() {
            let container_id = container.id.clone();
            self.remove_container_by_idx(container_idx);

            if !self.focus_after_scrolling_removal(&container_id) {
                self.focus_previous_container();
            }
        } else {
            container.load_focused_window(spot)?;
            if let Some(window) = container.focused_window() {
                window.focus(false)?;
            }
        }

        Ok(window)
    }

    fn restore(
        &mut self,
        mouse_follows_focus: bool,
        monitor_id: u32,
        monitor_wp: &Option<Wallpaper>,
    ) -> eyre::Result<()> {
        self.restore_windows(mouse_follows_focus)?;
        self.apply_wallpaper(monitor_id, monitor_wp)
    }

    fn restore_windows(&mut self, mouse_follows_focus: bool) -> eyre::Result<()> {
        if let Some(container) = &mut self.monocle_container {
            container.restore()?;

            if let Some(window) = container.focused_window() {
                window.focus(mouse_follows_focus)?;
            }

            return Ok(());
        }

        let idx = self.focused_container_idx();
        let mut to_focus = None;

        for (i, container) in self.containers().iter().enumerate() {
            if let Some(window) = container.focused_window()
                && idx == i
            {
                to_focus = Option::from(window.clone());
            }
        }

        for container in self.containers_mut() {
            container.restore()?;
        }

        if let Some(container) = self.focused_container_mut() {
            container.focus_window(container.focused_window_idx());
        }

        for window in self.floating_windows_mut() {
            window.restore()?;
        }
        // Do this here to make sure that an error doesn't stop the restoration of other windows
        // Maximised windows and floating windows should always be drawn at the top of the Z order
        // when switching to a workspace
        if let Some(window) = to_focus {
            if
            /* self.maximized_window().is_none() && */
            matches!(self.layer, WorkspaceLayer::Tiling) {
                window.focus(mouse_follows_focus)?;
            // } else if let Some(maximized_window) = self.maximized_window() {
            //     maximized_window.restore();
            //     maximized_window.focus(mouse_follows_focus)?;
            } else if let Some(floating_window) = self.focused_floating_window() {
                floating_window.focus(mouse_follows_focus)?;
            }
        // } else if let Some(maximized_window) = self.maximized_window() {
        //     maximized_window.restore();
        //     maximized_window.focus(mouse_follows_focus)?;
        } else if let Some(floating_window) = self.focused_floating_window() {
            floating_window.focus(mouse_follows_focus)?;
        }

        Ok(())
    }

    fn visible_window_details(&self) -> Vec<WindowDetails> {
        let mut vec: Vec<WindowDetails> = vec![];

        if let Some(maximized) = &self.maximized_window {
            let details = maximized.into();
            vec.push(details);
        }

        if let Some(monocle) = &self.monocle_container
            && let Some(focused) = monocle.focused_window()
        {
            let details = focused.into();
            vec.push(details);
        }

        for container in self.containers() {
            if let Some(focused) = container.focused_window() {
                let details = focused.into();
                vec.push(details);
            }
        }

        for window in self.floating_windows() {
            let details = window.into();
            vec.push(details);
        }

        vec
    }

    fn set_scrolling_columns(&mut self, columns: usize) {
        let mut options = self.layout_options.unwrap_or(LayoutOptions {
            scrolling: None,
            grid: None,
            column_ratios: None,
            row_ratios: None,
        });

        match &mut options.scrolling {
            Some(scrolling) => scrolling.columns = columns,
            None => {
                options.scrolling = Some(ScrollingLayoutOptions {
                    columns,
                    center_focused_column: None,
                });
            }
        }

        self.layout_options = Some(options);
    }

    fn calculate_layouts(
        &mut self,
        work_area: &Rect,
        container_padding: Option<i32>,
    ) -> eyre::Result<Vec<Rect>> {
        let effective_layout_options = self.effective_layout_options();

        tracing::debug!(
            "Workspace '{}' update() - effective_layout_options: {:?} (base: {:?}, rules: {})",
            self.name.as_deref().unwrap_or("unnamed"),
            effective_layout_options,
            self.layout_options,
            self.layout_options_rules.len(),
        );

        Ok(self.layout.as_boxed_arrangement().calculate(
            work_area,
            NonZeroUsize::new(self.containers().len()).ok_or_eyre(
                "there must be at least one container to calculate a workspace layout",
            )?,
            container_padding,
            self.layout_flip,
            &self.resize_dimensions,
            self.focused_container_idx(),
            effective_layout_options,
            &self.latest_layout,
        ))
    }

    fn update(&mut self) -> eyre::Result<()> {
        // make sure we are never holding on to empty containers
        self.prune_duplicate_windows()?;
        let focused_container_idx = self.focused_container_idx();
        self.containers_mut()
            .retain(|c| c.is_preselect() || !c.windows().is_empty());

        // make sure we are never focused on a no longer existent final empty container
        if focused_container_idx >= self.containers().len() {
            self.focus_container(focused_container_idx.saturating_sub(1));
        }

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
        let window_based_work_area_offset = self.globals.window_based_work_area_offset;
        let window_based_work_area_offset_limit = self.globals.window_based_work_area_offset_limit;

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

        if self.tile {
            // Batch screen updates for flicker-free window positioning
            let connection_id = unsafe { CGSMainConnectionID() };
            unsafe { SLSDisableUpdate(connection_id) };

            let result = (|| -> eyre::Result<()> {
                if let Some(container) = self.monocle_container.as_mut() {
                    if let Some(window) = container.focused_window_mut() {
                        adjusted_work_area.add_padding(container_padding);
                        adjusted_work_area.add_padding(border_offset);
                        adjusted_work_area.add_padding(border_width);
                        window.set_rect(&adjusted_work_area)?;
                    };
                } else if !self.containers().is_empty() {
                    if self.is_scrolling() {
                        return self.arrange_scrolling(
                            &work_area,
                            &adjusted_work_area,
                            container_padding,
                            border_offset,
                            border_width,
                        );
                    }

                    let mut layouts =
                        self.calculate_layouts(&adjusted_work_area, Some(container_padding))?;

                    let containers = self.containers_mut();

                    for (i, container) in containers.iter_mut().enumerate() {
                        if let Some(layout) = layouts.get_mut(i) {
                            // clamp layout to work area boundaries to prevent bleeding into adjacent monitors
                            clamp_to_work_area(layout, &adjusted_work_area);

                            layout.add_padding(border_offset);
                            layout.add_padding(border_width);

                            for window in container.windows_mut() {
                                if let Err(error) = window.set_rect(layout) {
                                    tracing::warn!("failed to set window position: {error}")
                                }
                            }
                        }
                    }

                    self.latest_layout = layouts;
                }
                Ok(())
            })();

            // Always re-enable updates, even if there was an error
            unsafe { SLSReenableUpdate(connection_id) };

            // Propagate any error that occurred
            result?;
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

    fn apply_wallpaper(&self, monitor_id: u32, monitor_wp: &Option<Wallpaper>) -> eyre::Result<()> {
        if let Some(wallpaper) = self.wallpaper.as_ref().or(monitor_wp.as_ref()) {
            if let Err(error) = MacosApi::set_wallpaper(&wallpaper.path, monitor_id) {
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
                    let komorebi_theme = KomorebiTheme::Custom(KomorebiThemeCustom {
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
                        bar_accent: wallpaper.theme_options.as_ref().and_then(|o| o.bar_accent),
                        stackbar_focused_text: None,
                        stackbar_unfocused_text: None,
                        stackbar_background: None,
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
}

/// Clamp a layout rect to the work area so that a window never bleeds into an adjacent monitor.
///
/// A rect that lies entirely outside the work area is left with an empty or negative size, so a
/// clamped rect says where a window is going to be drawn, not where its column sits in the strip.
pub(crate) fn clamp_to_work_area(layout: &mut Rect, work_area: &Rect) {
    // horizontal
    if layout.left < work_area.left {
        layout.right -= work_area.left - layout.left;
        layout.left = work_area.left;
    }
    let max_right_edge = work_area.left + work_area.right;
    let current_right_edge = layout.left + layout.right;
    if current_right_edge > max_right_edge {
        layout.right = max_right_edge - layout.left;
    }

    // vertical
    if layout.top < work_area.top {
        layout.bottom -= work_area.top - layout.top;
        layout.top = work_area.top;
    }
    let max_bottom_edge = work_area.top + work_area.bottom;
    let current_bottom_edge = layout.top + layout.bottom;
    if current_bottom_edge > max_bottom_edge {
        layout.bottom = max_bottom_edge - layout.top;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::Container;
    use crate::window::Window;
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
        ws.new_container_for_window(Window::from(123)).unwrap();

        // new length should be 5, with the focus on the new window at index 4
        assert_eq!(ws.containers().len(), 5);
        assert_eq!(ws.focused_container_idx(), 4);
        assert_eq!(
            ws.focused_container().unwrap().focused_window().unwrap().id,
            123
        );

        // when inserting a new container at index 0, index 3's container should not change
        ws.focus_container(0);
        ws.new_container_for_window(Window::from(234)).unwrap();
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
        assert_eq!(ws.containers()[0].focused_window().unwrap().id, 2);
        // index 1 should still be the same
        assert_eq!(ws.containers()[1].focused_window().unwrap().id, 1);
        assert_eq!(ws.containers()[2].focused_window().unwrap().id, 3);
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

        assert_eq!(ws.containers()[0].focused_window().unwrap().id, 2);
        // index 1 should still be the same
        assert_eq!(ws.containers()[1].focused_window().unwrap().id, 1);
        assert_eq!(ws.containers()[2].focused_window().unwrap().id, 3);

        // unfloat - have to do this semi-manually becuase of calls to WindowsApi in
        // new_container_for_floating_window which usually handles unfloating
        let window = ws.floating_windows_mut().pop_back().unwrap();
        let mut container = Container::default();
        container.add_window(window, Default::default()).unwrap();
        ws.insert_container_at_idx(ws.focused_container_idx(), container);

        // all indexes should be at their original position
        for i in 0..4 {
            assert_eq!(ws.containers()[i].focused_window().unwrap().id, i as u32);
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

        assert_eq!(ws.containers()[0].focused_window().unwrap().id, 0);
        assert_eq!(ws.containers()[1].focused_window().unwrap().id, 1);
        assert_eq!(ws.containers()[2].windows().len(), 2);
        assert_eq!(ws.containers()[3].focused_window().unwrap().id, 5);
        // index 4 should still be the same
        assert_eq!(ws.containers()[4].focused_window().unwrap().id, 4);

        // unstack
        ws.new_container_for_focused_window().unwrap();

        // all indexes should be at their original position
        for i in 0..6 {
            assert_eq!(ws.containers()[i].focused_window().unwrap().id, i as u32)
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
        workspace.new_container_for_window(Window::from(2)).unwrap();

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
        assert_eq!(focused_container.focused_window().unwrap().id, 1);
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
        let mut contains = false;
        for w in floating_windows {
            if w.id == 0 {
                contains = true;
            }
        }
        assert!(contains);
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
            assert_eq!(visible_windows[1].unwrap().id, 100);
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
            assert_eq!(visible_windows[1].unwrap().id, 100);
            assert_eq!(visible_windows[2].unwrap().id, 300);
        }

        // // Maximize window 200
        // workspace.maximized_window = Some(Window { hwnd: 200 });
        //
        // {
        //     // visible_windows should return 200, 100, and 300
        //     let visible_windows = workspace.visible_windows();
        //     assert_eq!(visible_windows.len(), 3);
        //     assert_eq!(visible_windows[0].unwrap().hwnd, 200);
        //     assert_eq!(visible_windows[1].unwrap().hwnd, 100);
        //     assert_eq!(visible_windows[2].unwrap().hwnd, 300);
        // }
    }

    #[test]
    fn test_layout_boundary_clamping() {
        use crate::core::Rect;
        use komorebi_layouts::default_layout::DefaultLayout;
        use std::num::NonZeroUsize;

        // Simulate an ultrawide monitor at 5120px width
        let work_area = Rect {
            left: 0,
            top: 0,
            right: 5120,
            bottom: 1440,
        };

        // Create a workspace with 2 containers in Columns layout
        let layout = Layout::Default(DefaultLayout::Columns);

        // Simulate a resize adjustment that would cause overflow
        // Container 0 gets expanded by 2100px to the right
        let resize_dimensions = vec![
            Some(Rect {
                left: 0,
                top: 0,
                right: 2100,
                bottom: 0,
            }),
            None,
        ];

        // Calculate the layout
        let mut layouts = layout.as_boxed_arrangement().calculate(
            &work_area,
            NonZeroUsize::new(2).unwrap(),
            None,
            None,
            &resize_dimensions,
            0,
            None,
            &[],
        );

        // Apply boundary clamping (mimicking the logic in workspace.rs update())
        for layout in &mut layouts {
            // Check horizontal boundaries
            if layout.left < work_area.left {
                layout.right -= work_area.left - layout.left;
                layout.left = work_area.left;
            }
            let max_right_edge = work_area.left + work_area.right;
            let current_right_edge = layout.left + layout.right;
            if current_right_edge > max_right_edge {
                layout.right = max_right_edge - layout.left;
            }

            // Check vertical boundaries
            if layout.top < work_area.top {
                layout.bottom -= work_area.top - layout.top;
                layout.top = work_area.top;
            }
            let max_bottom_edge = work_area.top + work_area.bottom;
            let current_bottom_edge = layout.top + layout.bottom;
            if current_bottom_edge > max_bottom_edge {
                layout.bottom = max_bottom_edge - layout.top;
            }
        }

        // Verify that all layouts stay within work area boundaries
        for layout in &layouts {
            // Horizontal bounds
            assert!(
                layout.left >= work_area.left,
                "Layout left ({}) should be >= work_area left ({})",
                layout.left,
                work_area.left
            );
            let right_edge = layout.left + layout.right;
            let max_right_edge = work_area.left + work_area.right;
            assert!(
                right_edge <= max_right_edge,
                "Layout right edge ({}) should be <= work_area right edge ({})",
                right_edge,
                max_right_edge
            );

            // Vertical bounds
            assert!(
                layout.top >= work_area.top,
                "Layout top ({}) should be >= work_area top ({})",
                layout.top,
                work_area.top
            );
            let bottom_edge = layout.top + layout.bottom;
            let max_bottom_edge = work_area.top + work_area.bottom;
            assert!(
                bottom_edge <= max_bottom_edge,
                "Layout bottom edge ({}) should be <= work_area bottom edge ({})",
                bottom_edge,
                max_bottom_edge
            );
        }

        // Additionally verify that with the resize adjustment,
        // the second container doesn't bleed beyond 5120
        assert_eq!(layouts.len(), 2);
        let container1_right_edge = layouts[1].left + layouts[1].right;
        assert!(
            container1_right_edge <= 5120,
            "Container 1 right edge ({}) should not exceed monitor width (5120)",
            container1_right_edge
        );
    }

    #[test]
    fn test_clamp_to_work_area() {
        let work_area = Rect {
            left: 100,
            top: 50,
            right: 1000,
            bottom: 600,
        };

        // partly past the left edge: the part inside the work area is what remains
        let mut layout = Rect {
            left: 0,
            top: 50,
            right: 400,
            bottom: 600,
        };
        clamp_to_work_area(&mut layout, &work_area);
        assert_eq!(
            layout,
            Rect {
                left: 100,
                top: 50,
                right: 300,
                bottom: 600
            }
        );

        // partly past the right edge
        let mut layout = Rect {
            left: 900,
            top: 50,
            right: 400,
            bottom: 600,
        };
        clamp_to_work_area(&mut layout, &work_area);
        assert_eq!(layout.left, 900);
        assert_eq!(layout.right, 200);

        // entirely past the left edge: no visible area is left, and the size goes negative
        let mut layout = Rect {
            left: -500,
            top: 50,
            right: 400,
            bottom: 600,
        };
        clamp_to_work_area(&mut layout, &work_area);
        assert_eq!(layout.left, 100);
        assert!(layout.right <= 0);
    }

    #[test]
    fn test_set_scrolling_columns() {
        let columns_of = |workspace: &Workspace| {
            workspace
                .layout_options
                .and_then(|options| options.scrolling)
                .map(|scrolling| scrolling.columns)
        };

        // a workspace that has no layout options at all
        let mut workspace = Workspace::default();
        workspace.set_scrolling_columns(4);
        assert_eq!(columns_of(&workspace), Some(4));

        // layout options for something else must not swallow the change
        let ratios = crate::core::validate_ratios(&[0.6]);
        let mut workspace = Workspace {
            layout_options: Some(LayoutOptions {
                scrolling: None,
                grid: None,
                column_ratios: Some(ratios),
                row_ratios: None,
            }),
            ..Default::default()
        };
        workspace.set_scrolling_columns(4);
        assert_eq!(columns_of(&workspace), Some(4));
        assert_eq!(
            workspace.layout_options.and_then(|o| o.column_ratios),
            Some(ratios)
        );

        // existing scrolling options keep their other settings
        let mut workspace = Workspace {
            layout_options: Some(LayoutOptions {
                scrolling: Some(ScrollingLayoutOptions {
                    columns: 2,
                    center_focused_column: Some(crate::core::ScrollingCentering::Always),
                }),
                grid: None,
                column_ratios: None,
                row_ratios: None,
            }),
            ..Default::default()
        };
        workspace.set_scrolling_columns(5);
        assert_eq!(columns_of(&workspace), Some(5));
        assert_eq!(
            workspace
                .layout_options
                .and_then(|o| o.scrolling)
                .and_then(|s| s.center_focused_column),
            Some(crate::core::ScrollingCentering::Always)
        );
    }
}

#[cfg(test)]
mod state_pin {
    use super::*;

    /// `komorebic state` prints a workspace with exactly these keys, whatever type it is: the
    /// macOS workspace is the shared `komorebi_core::Workspace`, which must not add or drop one.
    #[test]
    fn a_default_workspace_is_written_with_the_keys_it_always_had() {
        let value = serde_json::to_value(Workspace::default()).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();

        assert_eq!(
            keys,
            [
                "apply_window_based_work_area_offset",
                "container_padding",
                "containers",
                "float_override",
                "floating_layer_behaviour",
                "floating_windows",
                "latest_layout",
                "layer",
                "layout",
                "layout_flip",
                "layout_options",
                "layout_options_rules",
                "layout_rules",
                "maximized_window",
                "monocle_container",
                "name",
                "resize_dimensions",
                "tile",
                "wallpaper",
                "window_container_behaviour",
                "window_container_behaviour_rules",
                "work_area_offset",
                "work_area_offset_rules",
                "workspace_padding",
            ]
        );
    }
}
