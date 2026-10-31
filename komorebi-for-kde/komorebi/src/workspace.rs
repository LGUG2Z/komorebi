//! [`Workspace`], a ring of containers tiled by a layout.
//!
//! A workspace keeps its containers in order. [`Workspace::update`] asks the layout for one tile
//! per container in the work area it was given, less its padding, and puts every window of each
//! container on its tile with [`Window::set_position`]. The work area and the padding a workspace
//! falls back to come from the monitor it is on ([`WorkspaceGlobals`]).
//!
//! A workspace takes its name, layout, padding, offsets and rules from its entry in
//! `komorebi.json` ([`Workspace::load_static_config`]). Its layout and its offset can change with
//! the number of containers it has ([`Workspace::layout_rules`],
//! [`Workspace::work_area_offset_rules`]), and its layout options come from the workspace or from
//! the defaults of its layout.

use crate::HIDDEN_WINDOWS;
use crate::REGEX_IDENTIFIERS;
use crate::backend::backend;
use crate::container::Container;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::LayoutDefaultEntry;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::scrolling::WorkspaceScrolling;
use crate::static_config::Wallpaper;
use crate::static_config::WorkspaceConfig;
use crate::theme_manager;
use crate::window::PlatformWindowId;
use crate::window::Window;
use crate::window::WindowDetails;
use crate::window::should_act_window;
use crate::window::should_act_window_individual;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_backend::MonitorId;
pub use komorebi_core::WorkspaceGlobals;
use komorebi_themes::KomorebiTheme;
use komorebi_themes::KomorebiThemeCustom;
use std::collections::HashMap;
use std::num::NonZeroUsize;

use komorebi_core::CachedLayoutDefault;
pub use komorebi_core::WorkspaceLayer;
pub use komorebi_core::WorkspaceWindowLocation;
pub(crate) use komorebi_core::sorted_layout_options_rules;

/// The workspace over KDE's [`Window`], which is `komorebi_core::Workspace` with this platform's
/// methods added by [`WorkspaceExt`] and the scrolling ones of [`WorkspaceScrolling`]
pub type Workspace = komorebi_core::Workspace<KdePlatform>;

/// What only KDE keeps in a workspace
#[derive(Debug, Default, Clone, PartialEq)]
pub struct KdeWorkspaceState {
    /// Where the scrolling layout last placed the windows, and how wide it believed them
    /// ([`crate::scrolling::WIDTH_SETTLE`])
    pub(crate) scrolling_placed: Option<crate::scrolling::Placed>,
}

/// komorebi for KDE as the platform of the shared workspace
#[derive(Debug, Clone, PartialEq)]
pub struct KdePlatform;

impl komorebi_core::WorkspacePlatform for KdePlatform {
    type Window = Window;
    type State = KdeWorkspaceState;

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

    fn after_floating_removed(workspace: &mut Workspace) {
        workspace.focus_previous_floating_window();
    }

    fn unmaximize_window(window: &Window) -> eyre::Result<()> {
        Ok(window.unmaximize()?)
    }

    fn before_hide_containers(workspace: &mut Workspace) -> Option<Vec<Option<Rect>>> {
        // a scrolling workspace is tiled again without its resize adjustments first, so that the
        // windows are hidden where the layout has them and not where they were dragged; the
        // adjustments are put back after
        let mut resize_dimensions = None;
        if matches!(workspace.layout, Layout::Default(DefaultLayout::Scrolling))
            && workspace.resize_dimensions.iter().any(|r| r.is_some())
        {
            resize_dimensions = Some(workspace.resize_dimensions.clone());
            workspace.resize_dimensions = Vec::new();
            if let Err(error) = workspace.update() {
                tracing::error!("{error}");
            }
        }

        resize_dimensions
    }
}

/// `komorebi_layouts::Rect::percentage_within_horizontal_bounds`, which that crate compiles only
/// with its `darwin` feature
trait HorizontalBounds {
    /// Returns how much of `other`'s width is within this rectangle's left and right edges, in
    /// percent
    fn percentage_within_horizontal_bounds(&self, other: &Rect) -> f64;
}

impl HorizontalBounds for Rect {
    fn percentage_within_horizontal_bounds(&self, other: &Rect) -> f64 {
        let overlap_left = self.left.max(other.left);
        let overlap_right = (self.left + self.right).min(other.left + other.right);

        let overlap_width = overlap_right - overlap_left;

        if overlap_width <= 0 {
            0.0
        } else {
            (overlap_width as f64) / (other.right as f64) * 100.0
        }
    }
}

pub trait WorkspaceExt {
    /// Takes the settings of `config`, which `komorebi.json` has for this workspace: its name, its
    /// layout and the rules that change it with the number of containers, its padding and offsets,
    /// what a new window gets and whether it floats, whether it tiles, how its layout is flipped,
    /// what a new window does on its floating layer, and where in the layout a new window goes
    /// ([`Workspace::initial_window_placement_rules`]), and keeps `config` itself
    /// ([`Workspace::workspace_config`])
    ///
    /// `layout_defaults` is what `komorebi.json` says every layout has, for the layout options the
    /// workspace sets none of: it is copied here, sorted, so that [`Workspace::update`] has it at
    /// hand.
    ///
    /// A workspace that names no layout and does not say it tiles has tiling off, as the file gave
    /// it no layout to tile with, and one that has layout rules tiles.
    fn load_static_config(
        &mut self,
        config: &WorkspaceConfig,
        layout_defaults: Option<&HashMap<DefaultLayout, LayoutDefaultEntry>>,
    ) -> eyre::Result<()>;

    fn prune_duplicate_windows(&mut self) -> eyre::Result<()>;

    /// Remove every window that no longer exists
    fn reap_invalid_windows(&mut self) -> eyre::Result<usize>;

    /// Takes the window out of its container, and returns it. A container that is left with no
    /// window is removed. One that is left with windows shows the one in front of them now and
    /// gives it the focus in the compositor, so that closing the window in front of a stack brings
    /// the next one up.
    ///
    /// # Errors
    ///
    /// No container of the workspace has the window.
    fn remove_window(&mut self, window_id: u32) -> eyre::Result<Window>;

    /// Restores the window of every container that is in front, and the floating windows, and
    /// gives the focus to the window of this workspace that had it: the focused window of the
    /// focused container, or the focused floating window when there is no container.
    ///
    /// The compositor shows a workspace's windows by itself when the workspace's desktop is shown;
    /// this is what komorebi does after it has asked for that, and it brings back what komorebi
    /// itself has hidden (the back of a stack stays hidden). While there is a monocle container
    /// only its window is restored and focused, for the other containers are out of sight behind
    /// it. A maximized window is restored and given the focus ahead of the containers' windows.
    /// It sets the wallpaper the monitor is to show ([`Workspace::apply_wallpaper`]).
    fn restore(
        &mut self,
        mouse_follows_focus: bool,
        monitor_id: u32,
        monitor_wp: &Option<Wallpaper>,
    ) -> eyre::Result<()>;

    /// Takes the focused window out of its container, and so out of the layout, and has the
    /// compositor maximize it: the work area, panels respected, and a restore that goes back to
    /// where the window was. On the floating layer it is the focused floating window that is
    /// maximized, and the window of the monocle container is the focused one while there is a
    /// monocle container.
    ///
    /// # Errors
    ///
    /// The workspace has no containers, the focused one has no window, or the compositor refuses
    /// to maximize the window.
    fn new_maximized_window(&mut self) -> eyre::Result<()>;

    /// Takes a window the compositor has maximized without komorebi's being asked (the user's title
    /// bar, a double click, a shortcut) out of the layout, as [`Workspace::new_maximized_window`]
    /// does for the focused window: it becomes the workspace's maximized window, with the index it
    /// goes back to, until it is restored by `toggle-maximize` or by the user. The window is
    /// already maximized, so it is not asked to be.
    ///
    /// A window that is the maximized window already changes nothing. A workspace has room for one
    /// maximized window: the one it had goes back in the layout first, and a window that was
    /// floating or in the monocle container leaves it. The window goes back to the index of the
    /// focused container when it was floating, to the one the monocle container was taken from when
    /// it was in the monocle, and to its container's index otherwise. A stack or a monocle container
    /// that the window leaves shows the window that has the focus in it now, and hides the others.
    ///
    /// # Errors
    ///
    /// - The workspace has no such window in its floating windows, its monocle container or its
    ///   containers.
    /// - The maximized window it had cannot be put back, for one of the reasons
    ///   [`Workspace::reintegrate_maximized_window`] gives.
    fn adopt_maximized_window(&mut self, window_id: u32) -> eyre::Result<()>;

    /// The maximized window is a tile again, in a container of its own at the index it was taken
    /// from, and the compositor gives it back the frame it had
    ///
    /// # Errors
    ///
    /// The workspace has no maximized window, or the compositor refuses to restore it.
    fn reintegrate_maximized_window(&mut self) -> eyre::Result<()>;

    /// Tiles the workspace: asks the layout for a tile per container in the work area, less the
    /// offset (the workspace's own, or else the one its monitor gives it), the monitor's
    /// window-based offset while there are no more containers than its limit and
    /// [`Workspace::apply_window_based_work_area_offset`] is set, and the workspace padding, and
    /// puts every window of each container on its tile.
    ///
    /// The offset of the highest of [`Workspace::work_area_offset_rules`] that the number of
    /// containers reaches comes before the workspace's own, and a monocle container counts as one
    /// container for it. The highest of [`Workspace::layout_rules`] that it reaches makes the
    /// layout the workspace has from then on, and none that it does not reach changes it. The
    /// highest of [`Workspace::window_container_behaviour_rules`] that it reaches is the workspace's
    /// [`Workspace::window_container_behaviour`], which is none while it reaches none. The layout
    /// options are the workspace's own or, when it has none, those `layout_defaults` gives the
    /// layout, as [`Workspace::load_static_config`] copied them.
    ///
    /// Containers that have no windows left are dropped first, except the one a preselection
    /// reserved, and so is a window that is in more than one container. A tile is clamped to the
    /// work area, so it cannot bleed onto a neighboring monitor, except in the scrolling layout,
    /// whose strip runs past the edges of the work area. Each tile then gives up the room a border
    /// drawn by the compositor takes on every side, the border's width and its offset
    /// ([`WorkspaceGlobals`]; nothing when no border is drawn). A window whose tile is out of view
    /// while part of the window is in view is first centered in the work area. A workspace whose
    /// tiling is off ([`Workspace::tile`] is false) moves no window.
    ///
    /// While the workspace has a monocle container ([`Workspace::monocle_container`]), the window
    /// in front of it is put on the work area, less the container padding, and the other containers
    /// are left where they are. The window-based offset is then applied whatever the number of
    /// containers, if the monitor has one.
    ///
    /// While it has a maximized window ([`Workspace::maximized_window`]), the compositor is asked
    /// to maximize it again, which it does in its own work area, with no padding, and the
    /// containers are left where they are.
    ///
    /// The layout applies the resize adjustment of each container ([`Workspace::resize_dimensions`])
    /// after the parts its layout cannot apply have been cleared. The tiles are kept in
    /// [`Workspace::latest_layout`], which the scrolling layout scrolls from.
    fn update(&mut self) -> eyre::Result<()>;

    /// What the compositor says about each window [`Workspace::visible_windows`] has, as
    /// `komorebic visible-windows` shows it: the maximized window, the one at the top of the
    /// monocle container, the one at the top of each container, and every floating window. A
    /// container that has no window is left out.
    fn visible_window_details(&self) -> Vec<WindowDetails>;

    /// Has the monitor show this workspace's wallpaper, or else the monitor's `monitor_wp`, and
    /// gives the borders the colors of the palette of the picture unless the wallpaper asks for no
    /// theme. Nothing is done for a workspace and a monitor that have no wallpaper.
    ///
    /// A failure to set the wallpaper is logged and does not make this fail, and the colors are
    /// still changed: the picture may be shown a moment later, when the Plasma shell is up. The
    /// colors are changed before this returns, so the borders drawn for the workspace that comes
    /// into view are right at once.
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
    fn reap_invalid_windows(&mut self) -> eyre::Result<usize> {
        let mut invalid_window_ids = vec![];
        let mut invalid_floating_window_ids = vec![];

        for container in self.containers() {
            for window in container.windows() {
                if !window.is_valid() {
                    invalid_window_ids.push(window.id);
                }
            }
        }

        if let Some(container) = &self.monocle_container {
            for window in container.windows() {
                if !window.is_valid() {
                    invalid_window_ids.push(window.id);
                }
            }
        }

        for window in self.floating_windows() {
            if !window.is_valid() {
                invalid_floating_window_ids.push(window.id);
            }
        }

        for window_id in &invalid_window_ids {
            let _ = self.remove_window(*window_id);
        }

        for window_id in &invalid_floating_window_ids {
            self.remove_floating_window(*window_id);
        }

        tracing::debug!(
            "reaping invalid windows {invalid_window_ids:?} {invalid_floating_window_ids:?}"
        );

        Ok(invalid_window_ids.len() + invalid_floating_window_ids.len())
    }

    fn remove_window(&mut self, window_id: u32) -> eyre::Result<Window> {
        if let Some(window) = self.maximized_window
            && window.id == window_id
        {
            let _ = window.unmaximize();
            self.maximized_window = None;
            self.maximized_window_restore_idx = None;
            return Ok(window);
        }

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
                let _ = c.restore();
            }

            return Ok(window);
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
            let _ = container.load_focused_window(());
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
        if let Some(container) = &mut self.monocle_container {
            let _ = container.restore();

            if let Some(window) = container.focused_window() {
                window.focus(mouse_follows_focus)?;
            }

            return self.apply_wallpaper(monitor_id, monitor_wp);
        }

        let idx = self.focused_container_idx();
        let mut to_focus = None;

        for (i, container) in self.containers().iter().enumerate() {
            if let Some(window) = container.focused_window()
                && idx == i
            {
                to_focus = Option::from(*window);
            }
        }

        for container in self.containers_mut() {
            let _ = container.restore();
        }

        if let Some(container) = self.focused_container_mut() {
            container.focus_window(container.focused_window_idx());
        }

        for window in self.floating_windows_mut() {
            window.restore();
        }

        // Do this here to make sure that an error doesn't stop the restoration of other windows
        // Maximized windows and floating windows should always be drawn at the top of the Z order
        // when switching to a workspace
        if let Some(window) = to_focus {
            if self.maximized_window.is_none() && matches!(self.layer, WorkspaceLayer::Tiling) {
                window.focus(mouse_follows_focus)?;
            } else if let Some(maximized_window) = self.maximized_window {
                maximized_window.restore();
                maximized_window.focus(mouse_follows_focus)?;
            } else if let Some(floating_window) = self.focused_floating_window() {
                floating_window.focus(mouse_follows_focus)?;
            }
        } else if let Some(maximized_window) = self.maximized_window {
            maximized_window.restore();
            maximized_window.focus(mouse_follows_focus)?;
        } else if let Some(floating_window) = self.focused_floating_window() {
            floating_window.focus(mouse_follows_focus)?;
        }

        self.apply_wallpaper(monitor_id, monitor_wp)
    }

    fn new_maximized_window(&mut self) -> eyre::Result<()> {
        let focused_idx = self.focused_container_idx();

        if matches!(self.layer, WorkspaceLayer::Floating) {
            let window = self
                .remove_focused_floating_window()
                .ok_or_eyre("there is no floating window")?;

            self.maximized_window = Option::from(window);
            self.maximized_window_restore_idx = Option::from(focused_idx);
            window.maximize()?;

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
            window.maximize()?;

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
            // monocle and maximized toggles which take over the whole screen before being
            // reinserted at the same index to respect locked container indexes
            self.containers_mut().remove(focused_idx);
            if self.resize_dimensions.get(focused_idx).is_some() {
                self.resize_dimensions.remove(focused_idx);
            }
        } else {
            let _ = container.load_focused_window(());
        }

        self.maximized_window = Option::from(window);
        self.maximized_window_restore_idx = Option::from(focused_idx);
        window.maximize()?;

        Ok(())
    }

    fn adopt_maximized_window(&mut self, window_id: u32) -> eyre::Result<()> {
        if self.maximized_window.is_some_and(|w| w.id == window_id) {
            return Ok(());
        }

        // there is room for one: the one that was is a tile again
        if self.maximized_window.is_some() {
            self.reintegrate_maximized_window()?;
        }

        let focused_idx = self.focused_container_idx();

        if let Some(window) = self.remove_floating_window(window_id) {
            self.maximized_window = Option::from(window);
            self.maximized_window_restore_idx = Option::from(focused_idx);
            return Ok(());
        }

        let monocle_restore_idx = self.monocle_container_restore_idx;
        if let Some(monocle_container) = &mut self.monocle_container
            && let Some(window_idx) = monocle_container
                .windows()
                .iter()
                .position(|window| window.id == window_id)
        {
            let window = monocle_container
                .remove_window_by_idx(window_idx)
                .ok_or_eyre("there is no window")?;

            if monocle_container.windows().is_empty() {
                self.monocle_container = None;
                self.monocle_container_restore_idx = None;
            } else {
                let _ = monocle_container.load_focused_window(());
            }

            self.maximized_window = Option::from(window);
            self.maximized_window_restore_idx = monocle_restore_idx;
            return Ok(());
        }

        let container_idx = self
            .container_idx_for_window(window_id)
            .ok_or_eyre("there is no such window to maximize")?;

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
            // as for `toggle-maximize`: the index is kept free for the window to come back to
            self.containers_mut().remove(container_idx);
            if self.resize_dimensions.get(container_idx).is_some() {
                self.resize_dimensions.remove(container_idx);
            }
        } else {
            let _ = container.load_focused_window(());
        }

        self.maximized_window = Option::from(window);
        self.maximized_window_restore_idx = Option::from(container_idx);

        Ok(())
    }

    fn reintegrate_maximized_window(&mut self) -> eyre::Result<()> {
        let restore_idx = self
            .maximized_window_restore_idx
            .ok_or_eyre("there is no maximized restore index")?;

        let window = self
            .maximized_window
            .ok_or_eyre("there is no maximized window")?;

        // it is its tile's size that it has to get back, not the compositor's idea of its old size
        window.unmaximize()?;

        if !self.containers().is_empty() && restore_idx > self.containers().len().saturating_sub(1)
        {
            self.containers_mut()
                .resize(restore_idx, Container::default());
        }

        let mut container = Container::default();
        let _ = container.add_window(window, ());

        // we shouldn't use insert_container_at_index here because it doesn't make sense for
        // monocle and maximized toggles which take over the whole screen before being reinserted
        // at the same index to respect locked container indexes
        self.containers_mut().insert(restore_idx, container);
        self.focus_container(restore_idx);

        self.maximized_window = None;
        self.maximized_window_restore_idx = None;

        Ok(())
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
            // the backend batches the commands this produces and sends them together
            let result = (|| -> eyre::Result<()> {
                if let Some(container) = self.monocle_container.as_mut() {
                    if let Some(window) = container.focused_window_mut() {
                        adjusted_work_area.add_padding(container_padding);
                        adjusted_work_area.add_padding(border_offset);
                        adjusted_work_area.add_padding(border_width);
                        window.set_position(&adjusted_work_area)?;
                    };
                } else if let Some(window) = self.maximized_window {
                    // the compositor's own maximize, which is the work area and not the padded one
                    window.maximize()?;
                } else if !self.containers().is_empty() {
                    let is_scrolling = self.is_scrolling();
                    let around_window = container_padding + border_offset + border_width;

                    // a window that would not get as narrow as its column was learned of once the
                    // windows have had time to draw at the sizes that they were sent, and not
                    // while one is being dragged, whose neighbours are on their way
                    if is_scrolling
                        && self
                            .state
                            .scrolling_placed
                            .as_ref()
                            .is_some_and(crate::scrolling::Placed::has_settled)
                        && !crate::window::live_resize_in_progress()
                    {
                        self.adopt_actual_widths(around_window, adjusted_work_area.right);
                    }

                    let effective_layout_options = self.effective_layout_options();

                    tracing::debug!(
                        "Workspace '{}' update() - effective_layout_options: {:?} (base: {:?}, rules: {})",
                        self.name.as_deref().unwrap_or("unnamed"),
                        effective_layout_options,
                        self.layout_options,
                        self.layout_options_rules.len(),
                    );
                    // the containers that rest where another monitor is seen from, which are hidden
                    let mut hidden = vec![];
                    let mut layouts = if is_scrolling {
                        let monitors: Vec<Rect> = backend()
                            .monitors()
                            .map(|monitors| monitors.iter().map(|monitor| monitor.size).collect())
                            .unwrap_or_default();
                        let others =
                            crate::scrolling::other_monitors(&self.globals.work_area, &monitors);

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

                    let resize_dimensions_is_empty = self.resize_dimensions.is_empty();

                    let containers = self.containers_mut();

                    for (i, container) in containers.iter_mut().enumerate() {
                        if let Some(layout) = layouts.get_mut(i) {
                            // The scrolling layout is a strip that is meant to run past the edges of
                            // the work area, and says itself where each window rests when it is out
                            // of view (KWin does not constrain where a window is put). Everything
                            // else is clamped to the work area so it cannot bleed onto an adjacent
                            // monitor.
                            if !is_scrolling {
                                // horizontal
                                if layout.left < adjusted_work_area.left {
                                    layout.right -= adjusted_work_area.left - layout.left;
                                    layout.left = adjusted_work_area.left;
                                }
                                let max_right_edge =
                                    adjusted_work_area.left + adjusted_work_area.right;
                                let current_right_edge = layout.left + layout.right;
                                if current_right_edge > max_right_edge {
                                    layout.right = max_right_edge - layout.left;
                                }

                                // vertical
                                if layout.top < adjusted_work_area.top {
                                    layout.bottom -= adjusted_work_area.top - layout.top;
                                    layout.top = adjusted_work_area.top;
                                }
                                let max_bottom_edge =
                                    adjusted_work_area.top + adjusted_work_area.bottom;
                                let current_bottom_edge = layout.top + layout.bottom;
                                if current_bottom_edge > max_bottom_edge {
                                    layout.bottom = max_bottom_edge - layout.top;
                                }
                            }

                            layout.add_padding(border_offset);
                            layout.add_padding(border_width);

                            if hidden.get(i).copied().unwrap_or_default() {
                                // it is seen on another monitor wherever it rests here
                                for window in container.windows() {
                                    window.hide();
                                }
                                continue;
                            }

                            // a window that was hidden for resting past an edge with another
                            // monitor next to it is shown before it is put where the strip says
                            let front = container.focused_window().map(|window| window.id);
                            let restore = is_scrolling
                                && front.is_some_and(|front| {
                                    crate::scrolling::should_restore_when_placed(
                                        front,
                                        Some(front),
                                        &HIDDEN_WINDOWS.lock(),
                                    )
                                });
                            if restore && let Some(window) = container.focused_window() {
                                window.restore();
                            }

                            for window in container.windows_mut() {
                                let current_rect = window.rect().unwrap_or_default();
                                let current_percentage =
                                    work_area.percentage_within_horizontal_bounds(&current_rect);
                                let proposed_percentage =
                                    work_area.percentage_within_horizontal_bounds(layout);

                                // the strip puts a window where it goes, however far
                                let percentage_override = !is_scrolling
                                    && proposed_percentage == 0.0
                                    && current_percentage > 0.1
                                    && current_percentage < 100.0
                                    && resize_dimensions_is_empty;

                                if percentage_override {
                                    window.center(&work_area, false)?;
                                } else if let Err(error) = window.set_position(layout) {
                                    tracing::warn!("failed to set window position: {error}")
                                }
                            }
                        }
                    }

                    self.latest_layout = layouts;

                    if is_scrolling {
                        self.state.scrolling_placed = Some(crate::scrolling::Placed::new(
                            self.containers()
                                .iter()
                                .zip(self.latest_layout.iter())
                                .map(|(container, layout)| (container.id.clone(), layout.right)),
                        ));
                    }
                }
                Ok(())
            })();

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

    fn apply_wallpaper(&self, monitor_id: u32, monitor_wp: &Option<Wallpaper>) -> eyre::Result<()> {
        if let Some(wallpaper) = self.wallpaper.as_ref().or(monitor_wp.as_ref()) {
            if let Err(error) =
                backend().set_wallpaper(MonitorId::from_u32(monitor_id), &wallpaper.path)
            {
                tracing::error!("failed to set wallpaper: {error}");
            }

            if wallpaper.generate_theme.unwrap_or(true) {
                let options = wallpaper.theme_options.as_ref();
                let variant = options.and_then(|o| o.theme_variant).unwrap_or_default();

                if let Some(palette) = theme_manager::wallpaper_palette(&wallpaper.path, variant) {
                    theme_manager::send_notification(KomorebiTheme::Custom(KomorebiThemeCustom {
                        colours: Box::new(palette),
                        single_border: options.and_then(|o| o.single_border),
                        stack_border: options.and_then(|o| o.stack_border),
                        monocle_border: options.and_then(|o| o.monocle_border),
                        floating_border: options.and_then(|o| o.floating_border),
                        unfocused_border: options.and_then(|o| o.unfocused_border),
                        unfocused_locked_border: options.and_then(|o| o.unfocused_locked_border),
                        bar_accent: options.and_then(|o| o.bar_accent),
                        stackbar_focused_text: None,
                        stackbar_unfocused_text: None,
                        stackbar_background: None,
                    }));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use crate::container::Container;
    use crate::window::Window;
    use std::collections::HashMap;
    use std::sync::Arc;

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
            ws.focused_container().unwrap().focused_window().unwrap().id,
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
        let _ = container.add_window(window, ());
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

        // Maximize window 200
        workspace.maximized_window = Some(Window::from(200));

        {
            // visible_windows should return 200, 100, and 300
            let visible_windows = workspace.visible_windows();
            assert_eq!(visible_windows.len(), 3);
            assert_eq!(visible_windows[0].unwrap().id, 200);
            assert_eq!(visible_windows[1].unwrap().id, 100);
            assert_eq!(visible_windows[2].unwrap().id, 300);
        }
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

    fn tile(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// A workspace on the fake's 1920x1080 monitor, whose work area is 1920x1050, with a container
    /// for each of `ids`
    fn workspace_with(ids: &[u32]) -> (Arc<FakeBackend>, Workspace) {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        let mut workspace = Workspace::default();
        workspace.globals.work_area = tile(0, 0, 1920, 1050);

        for id in ids {
            fake.open_window(*id, "foot", "foot", &format!("terminal {id}"));
            let _ = workspace.new_container_for_window(Window::from(*id));
        }

        (fake, workspace)
    }

    #[test]
    fn update_puts_each_window_on_its_tile() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with(&[401, 402]);

        with_backend(fake.clone(), || workspace.update())?;

        assert_eq!(fake.rect_of(401), Some(tile(0, 0, 960, 1050)));
        assert_eq!(fake.rect_of(402), Some(tile(960, 0, 960, 1050)));
        Ok(())
    }

    #[test]
    fn update_leaves_the_padding_and_the_offset_free() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with(&[411]);
        workspace.workspace_padding = Some(10);
        workspace.container_padding = Some(5);
        workspace.globals.work_area_offset = Some(tile(0, 40, 0, 40));

        with_backend(fake.clone(), || workspace.update())?;

        // the offset moves the top edge down by 40, the workspace padding takes 10 off every edge,
        // and the container padding 5 more
        assert_eq!(fake.rect_of(411), Some(tile(15, 55, 1890, 980)));
        Ok(())
    }

    /// A workspace with the windows `ids` in containers of their own, and the last two of them
    /// stacked: the last is in front, and the one before it hidden
    fn workspace_with_a_stack(ids: &[u32]) -> (Arc<FakeBackend>, Workspace) {
        let (fake, mut workspace) = workspace_with(ids);
        let last = ids.len() - 1;
        with_backend(fake.clone(), || {
            workspace.focus_container(last);
            workspace.move_window_to_container(last - 1).unwrap();
        });
        (fake, workspace)
    }

    #[test]
    fn stacking_a_window_puts_it_in_front_and_hides_the_one_it_was_put_on() {
        let (fake, workspace) = workspace_with_a_stack(&[431, 432, 433]);

        assert_eq!(workspace.containers().len(), 2);
        let stack = workspace.focused_container().unwrap();
        assert_eq!(stack.focused_window().map(|w| w.id), Some(433));
        assert!(fake.is_hidden(432) && !fake.is_hidden(433) && !fake.is_hidden(431));
    }

    #[test]
    fn a_window_that_leaves_a_stack_shows_the_next_one() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with_a_stack(&[441, 442, 443]);

        with_backend(fake.clone(), || workspace.remove_window(443))?;

        assert!(!fake.is_hidden(442));
        assert_eq!(workspace.containers().len(), 2);
        assert_eq!(workspace.focused_container().unwrap().windows().len(), 1);
        Ok(())
    }

    #[test]
    fn focusing_a_hidden_window_of_a_stack_brings_it_to_the_front() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with_a_stack(&[451, 452, 453]);
        assert!(fake.is_hidden(452));

        with_backend(fake.clone(), || workspace.focus_container_by_window(452))?;

        assert!(!fake.is_hidden(452) && fake.is_hidden(453));
        Ok(())
    }

    #[test]
    fn showing_the_workspace_again_restores_the_front_of_each_stack_and_no_other()
    -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with_a_stack(&[461, 462, 463]);

        with_backend(fake.clone(), || workspace.restore(false, 0, &None))?;

        assert!(fake.is_hidden(462), "the back of the stack stays hidden");
        assert!(!fake.is_hidden(463) && !fake.is_hidden(461));
        Ok(())
    }

    #[test]
    fn hiding_a_workspace_hides_every_window_but_the_one_it_is_told_to_leave() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with_a_stack(&[471, 472, 473]);

        let _ = with_backend(fake.clone(), || workspace.hide(Some(471)));
        assert!(!fake.is_hidden(471) && fake.is_hidden(472) && fake.is_hidden(473));

        let _ = with_backend(fake.clone(), || workspace.hide(None));
        assert!(fake.is_hidden(471));
        Ok(())
    }

    /// A workspace with the windows `ids` in containers of their own, and the last of them in
    /// monocle: the others are hidden
    fn workspace_in_monocle(ids: &[u32]) -> (Arc<FakeBackend>, Workspace) {
        let (fake, mut workspace) = workspace_with(ids);
        with_backend(fake.clone(), || {
            workspace.new_monocle_container().unwrap();
            for container in workspace.containers_mut() {
                let _ = container.hide(None, ());
            }
        });
        (fake, workspace)
    }

    #[test]
    fn showing_a_workspace_in_monocle_restores_the_monocle_window_and_no_other() -> eyre::Result<()>
    {
        let (fake, mut workspace) = workspace_in_monocle(&[481, 482, 483]);
        assert!(fake.is_hidden(481) && fake.is_hidden(482) && !fake.is_hidden(483));
        let _ = with_backend(fake.clone(), || workspace.hide(None));
        assert!(
            fake.is_hidden(483),
            "leaving the workspace hides the monocle window too"
        );

        with_backend(fake.clone(), || workspace.restore(false, 0, &None))?;

        assert!(!fake.is_hidden(483), "the window in monocle is shown");
        assert!(
            fake.is_hidden(481) && fake.is_hidden(482),
            "the containers behind it stay hidden"
        );
        Ok(())
    }

    #[test]
    fn hiding_a_workspace_in_monocle_hides_the_monocle_window_too() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_in_monocle(&[491, 492]);

        let _ = with_backend(fake.clone(), || workspace.hide(None));

        assert!(fake.is_hidden(491) && fake.is_hidden(492));
        Ok(())
    }

    #[test]
    fn a_window_in_monocle_that_leaves_shows_the_containers_behind_it() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_in_monocle(&[501, 502, 503]);

        with_backend(fake.clone(), || workspace.remove_window(503))?;

        assert!(workspace.monocle_container.is_none());
        assert!(!fake.is_hidden(501) && !fake.is_hidden(502));
        Ok(())
    }

    #[test]
    fn update_drops_a_container_that_has_no_windows_left() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with(&[421]);
        workspace.add_container_to_back(Container::default());

        with_backend(fake.clone(), || workspace.update())?;

        assert_eq!(workspace.containers().len(), 1);
        assert_eq!(workspace.focused_container_idx(), 0);
        assert_eq!(fake.rect_of(421), Some(tile(0, 0, 1920, 1050)));
        Ok(())
    }

    fn loaded(json: &str) -> eyre::Result<Workspace> {
        let config: WorkspaceConfig = serde_json::from_str(json)?;
        let mut workspace = Workspace::default();
        workspace.load_static_config(&config, None)?;
        Ok(workspace)
    }

    #[test]
    fn layout_rules_are_kept_in_the_order_of_their_thresholds_and_turn_tiling_on()
    -> eyre::Result<()> {
        let workspace = loaded(
            r#"{ "name": "rules", "tile": false, "layout_rules": { "3": "Rows", "2": "Columns" } }"#,
        )?;

        assert_eq!(
            workspace.layout_rules,
            [
                (2, Layout::Default(DefaultLayout::Columns)),
                (3, Layout::Default(DefaultLayout::Rows))
            ]
        );
        assert!(workspace.tile);
        assert_eq!(workspace.name.as_deref(), Some("rules"));
        Ok(())
    }

    #[test]
    fn a_workspace_tiles_when_it_has_a_layout_or_is_told_to() -> eyre::Result<()> {
        assert!(!loaded(r#"{ "name": "no layout" }"#)?.tile);
        assert!(loaded(r#"{ "name": "layout", "layout": "Grid" }"#)?.tile);
        assert!(loaded(r#"{ "name": "told to", "tile": true }"#)?.tile);
        assert!(!loaded(r#"{ "name": "told not to", "layout": "Grid", "tile": false }"#)?.tile);

        // the layout is the workspace's own when it names one, and what it had when it does not
        assert_eq!(
            loaded(r#"{ "name": "grid", "layout": "Grid" }"#)?.layout,
            Layout::Default(DefaultLayout::Grid)
        );
        assert_eq!(
            loaded(r#"{ "name": "none" }"#)?.layout,
            Layout::Default(DefaultLayout::BSP)
        );
        Ok(())
    }

    #[test]
    fn the_offset_of_the_highest_rule_the_containers_reach_comes_before_the_workspaces_own()
    -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with(&[431, 432]);
        workspace.workspace_padding = Some(0);
        workspace.container_padding = Some(0);
        workspace.work_area_offset = Some(tile(0, 100, 0, 100));
        workspace.work_area_offset_rules = vec![(1, tile(10, 0, 20, 0)), (2, tile(40, 0, 80, 0))];

        with_backend(fake.clone(), || workspace.update())?;

        // two containers reach both rules, and the second is the higher: 40 on the left, and 80
        // narrower
        assert_eq!(fake.rect_of(431), Some(tile(40, 0, 920, 1050)));
        assert_eq!(fake.rect_of(432), Some(tile(960, 0, 920, 1050)));

        workspace.remove_container_by_idx(1);
        with_backend(fake.clone(), || workspace.update())?;
        assert_eq!(fake.rect_of(431), Some(tile(10, 0, 1900, 1050)));

        workspace.work_area_offset_rules.clear();
        with_backend(fake.clone(), || workspace.update())?;
        assert_eq!(fake.rect_of(431), Some(tile(0, 100, 1920, 950)));
        Ok(())
    }
}
