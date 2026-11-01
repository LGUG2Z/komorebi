//! [`Workspace`], a ring of containers tiled by a layout, generic over the platform.
//!
//! A workspace keeps its containers in order, with a monocle container, a maximized window and the
//! floating windows beside them. This module holds what is the same on every platform: the
//! containers and their focus, the layout choice and the placement of new windows. What is not the
//! same (laying the windows out on the screen, hiding and restoring them, the wallpaper) stays in
//! the daemon, which implements [`WorkspacePlatform`] and adds its own methods to the alias it
//! uses for `Workspace<ItsPlatform>`.

use crate::Container;
use crate::ContainerWindow;
use crate::WorkspaceGlobals;
use crate::WorkspaceWindow;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_collections::lockable_sequence::LockableSequence;
use komorebi_collections::ring::Ring;
use komorebi_config::behaviour::FloatingLayerBehaviour;
use komorebi_config::behaviour::WindowContainerBehaviour;
use komorebi_config::matching::IdWithIdentifier;
use komorebi_config::matching::MatchingRule;
use komorebi_config::placement::InitialWindowPlacementRules;
use komorebi_config::placement::PlacementMatchingRules;
use komorebi_config::placement::PlacementTarget;
use komorebi_config::placement::WindowPlacement;
use komorebi_config::static_config::Wallpaper;
use komorebi_config::static_config::WorkspaceConfig;
use komorebi_layouts::Axis;
use komorebi_layouts::CycleDirection;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::Layout;
use komorebi_layouts::LayoutOptions;
use komorebi_layouts::OperationDirection;
use komorebi_layouts::Rect;
use komorebi_scrolling::ScrollingSpace;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::num::NonZeroUsize;

/// What [`WorkspacePlatform::begin_update`] answers.
pub enum Flow {
    /// Do not update the workspace now.
    Skip,
    /// Update it, holding the guard (if any) until the update is done.
    Run(Option<Box<dyn std::any::Any>>),
}

/// What [`WorkspacePlatform::end_update`] answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// The windows are placed.
    Done,
    /// Lay the workspace out and place the windows once more.
    Again,
}

/// What the hooks of an update know about the pass that is running.
#[derive(Clone, Copy, Debug)]
pub struct Placement<'a> {
    /// The work area less the monitor's and the workspace's offsets and padding.
    pub adjusted_work_area: Rect,
    /// The work area of the monitor.
    pub work_area: Rect,
    /// The padding around a container.
    pub container_padding: i32,
    /// The offset of the border from the window.
    pub border_offset: i32,
    /// The width of the border.
    pub border_width: i32,
    /// Whether the workspace has the scrolling layout.
    pub is_scrolling: bool,
    /// Whether the workspace has no resize adjustments.
    pub resize_dimensions_is_empty: bool,
    /// Whether a window is maximized on this workspace.
    pub managed_maximized_window: bool,
    /// What [`WorkspacePlatform::begin_pass`] returned for this pass.
    pub pass: Option<&'a dyn std::any::Any>,
}

/// What names a window of the platform `P`: the id the shared code compares.
pub type PlatformWindowId<P> = <<P as WorkspacePlatform>::Window as ContainerWindow>::Id;

/// What a window of the platform `P` is hidden at.
pub type PlatformHideSpot<P> = <<P as WorkspacePlatform>::Window as ContainerWindow>::HideSpot;

/// What the shared [`Workspace`] needs of a platform.
pub trait WorkspacePlatform: Sized + Clone + std::fmt::Debug + PartialEq + 'static {
    /// The platform's window handle.
    type Window: WorkspaceWindow;

    /// What only this platform keeps in a workspace, skipped when the workspace is serialised.
    type State: Default + Clone + std::fmt::Debug + PartialEq;

    /// The container and workspace padding a new workspace starts with, in that order.
    fn default_padding() -> (Option<i32>, Option<i32>) {
        (None, None)
    }

    /// Where a window of this workspace is hidden (macOS: the corner of the monitor it is on, kept in
    /// [`WorkspacePlatform::State`]).
    /// Every shared function that hides or stacks a window asks for it, so the platform needs no
    /// other way to know the monitor.
    fn hide_spot(_workspace: &Workspace<Self>) -> PlatformHideSpot<Self> {
        Default::default()
    }

    /// Where the containers of a workspace that is being hidden ([`Workspace::hide`]) are hidden,
    /// when the platform knows the display of the monitor (macOS passes its bounds, so that a window
    /// that hangs over the edge of a display is parked on the display of its monitor). The floating
    /// windows of the workspace are hidden at [`WorkspacePlatform::hide_spot`].
    fn hide_spot_on_display(workspace: &Workspace<Self>) -> PlatformHideSpot<Self> {
        Self::hide_spot(workspace)
    }

    /// The id of the window that has the focus, if any window does.
    fn foreground_window_id() -> Option<PlatformWindowId<Self>>;

    /// Where the pointer is, if the platform says.
    fn cursor_position() -> Option<(i32, i32)>;

    /// Which of `rules` match a window with these properties (the platform holds the compiled
    /// regular expressions), as [`should_act`](https://docs.rs) does.
    fn should_act_window(
        title: &str,
        exe_name: &str,
        class: &str,
        path: &str,
        rules: &[MatchingRule],
    ) -> Option<MatchingRule>;

    /// Whether one rule matches a window with these properties.
    fn should_act_window_individual(
        title: &str,
        exe_name: &str,
        class: &str,
        path: &str,
        rule: &IdWithIdentifier,
    ) -> bool;

    /// The index the scrolling layout would move the focus to in `direction`.
    fn scrolling_idx_for_direction(
        workspace: &Workspace<Self>,
        direction: OperationDirection,
    ) -> Option<usize>;

    /// The index at which the scrolling layout would insert a new container.
    fn scrolling_insertion_idx(workspace: &Workspace<Self>) -> Option<usize>;

    /// Tells the scrolling layout that the container at `idx` is about to leave the workspace.
    fn note_scrolling_departure(workspace: &Workspace<Self>, idx: usize);

    /// Called by [`Workspace::remove_floating_window`] before the floating window is taken out of the
    /// workspace, while it is still in the ring: KDE and macOS move the focus to the previous
    /// floating window first (the focus index moves back by one whatever the length), Windows does
    /// nothing.
    fn before_floating_removed(_workspace: &mut Workspace<Self>) {}

    /// The index at which [`Workspace::promote_container`] inserts the container: the primary
    /// position of the layout. The default layouts have it first.
    fn primary_container_idx(_layout: &Layout) -> eyre::Result<usize> {
        Ok(0)
    }

    /// The index of the container that `promote-focus` focuses: the primary position of the layout,
    /// and the first container when the layout has none (a custom layout without a primary
    /// column). Where [`WorkspacePlatform::primary_container_idx`] fails, the default falls back
    /// to 0.
    fn promote_focus_container_idx(layout: &Layout) -> usize {
        Self::primary_container_idx(layout).unwrap_or(0)
    }

    /// Gives the maximized window back to the layout, for the platforms where that is something
    /// to tell the window (KDE asks the compositor; on Windows there is nothing to do).
    fn unmaximize_window(_window: &Self::Window) -> eyre::Result<()> {
        Ok(())
    }

    /// Called by [`Workspace::hide`] before it hides the containers. A platform that has to put
    /// the workspace in order first does it here (KDE tiles a scrolling workspace again without its
    /// resize adjustments, so that the windows are hidden where the layout has them) and returns
    /// the adjustments to put back.
    fn before_hide_containers(_workspace: &mut Workspace<Self>) -> Option<Vec<Option<Rect>>> {
        None
    }

    // ---- the hooks of `update_shared`, in the order it calls them -----------------------------

    /// Called first: the platform may skip the update (`Flow::Skip`) or run it holding a guard
    /// that is dropped when the update ends (Windows: the animation batch). KDE prunes duplicate
    /// windows here. Not called by a platform that has its own update.
    fn begin_update(_workspace: &mut Workspace<Self>) -> eyre::Result<Flow> {
        Ok(Flow::Run(None))
    }

    /// Called after the empty containers are dropped, with the index that had the focus before.
    /// KDE keeps the focus on a container that exists; Windows leaves it where it is.
    fn containers_pruned(_workspace: &mut Workspace<Self>, _focused_before: usize) {}

    /// Puts the monocle window on the work area it is given.
    fn place_monocle(_window: &mut Self::Window, _rect: &Rect) -> eyre::Result<()> {
        Err(eyre::eyre!("this platform has no shared update"))
    }

    /// Lays the maximized window out: the platform's own maximize.
    fn maximize_in_layout(_window: &mut Self::Window) -> eyre::Result<()> {
        Err(eyre::eyre!("this platform has no shared update"))
    }

    /// Whether the scrolling layout should learn the widths the windows really have before it is
    /// laid out (Windows: animations on and no live resize; KDE: the last placement has settled).
    fn adopt_widths_before_layout(_workspace: &Workspace<Self>) -> bool {
        false
    }

    /// Has the scrolling layout learn the widths the windows really have; true if one changed.
    fn adopt_actual_widths(
        _workspace: &mut Workspace<Self>,
        _around_window: i32,
        _viewport_width: i32,
    ) -> bool {
        false
    }

    /// The tile of every container of a scrolling workspace and whether each rests hidden past an
    /// edge with another monitor next to it.
    fn scrolling_layouts(
        _workspace: &mut Workspace<Self>,
        _adjusted_work_area: &Rect,
        _container_padding: i32,
    ) -> (Vec<Rect>, Vec<bool>) {
        (vec![], vec![])
    }

    /// Called once per placement pass, after the layout is computed and before the containers are
    /// placed: whatever the platform needs to read once for the whole pass (Windows takes a snapshot
    /// of the title-bar rules here, so that two containers of a pass see the same one). The answer
    /// is handed to the hooks of the pass as [`Placement::pass`].
    fn begin_pass(_workspace: &Workspace<Self>) -> Option<Box<dyn std::any::Any>> {
        None
    }

    /// Adjusts the tile of one container before its windows are placed: the room a border takes,
    /// and whatever else the platform needs (the clamp to the work area, the stackbar).
    fn adjust_layout(
        _container: &mut Container<Self::Window>,
        _layout: &mut Rect,
        _placement: &Placement<'_>,
    ) {
    }

    /// Puts the windows of a container on its (adjusted) tile, or hides them when `hidden`.
    fn place_container(
        _container: &mut Container<Self::Window>,
        _layout: &Rect,
        _hidden: bool,
        _placement: &Placement<'_>,
    ) -> eyre::Result<()> {
        Err(eyre::eyre!("this platform has no shared update"))
    }

    /// Called after every placement pass; `Pass::Again` runs one more (the Windows scrolling layout
    /// does once, when a window refused the width it was given).
    fn end_update(
        _workspace: &mut Workspace<Self>,
        _pass: usize,
        _around_window: i32,
        _viewport_width: i32,
    ) -> eyre::Result<Pass> {
        Ok(Pass::Done)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(bound(
    serialize = "P::Window: Serialize",
    deserialize = "P::Window: Deserialize<'de>"
))]
#[cfg_attr(
    feature = "schemars",
    schemars(rename = "Workspace", bound = "P::Window: schemars::JsonSchema")
)]
pub struct Workspace<P: WorkspacePlatform> {
    pub name: Option<String>,
    pub containers: Ring<Container<P::Window>>,
    /// The container that has the whole work area, and so is the only one in view: it is not one
    /// of `containers`, which are not given tiles until it is put back
    /// ([`Workspace::new_monocle_container`], [`Workspace::reintegrate_monocle_container`])
    pub monocle_container: Option<Container<P::Window>>,
    /// The index in `containers` the monocle container goes back to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monocle_container_restore_idx: Option<usize>,
    /// The window the compositor has maximized, which is in no container: the containers are left
    /// where they are, behind it ([`Workspace::new_maximized_window`],
    /// [`Workspace::reintegrate_maximized_window`])
    pub maximized_window: Option<P::Window>,
    /// The index in `containers` the maximized window goes back to, in a container of its own
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximized_window_restore_idx: Option<usize>,
    pub floating_windows: Ring<P::Window>,
    pub layout: Layout,
    pub layout_options: Option<LayoutOptions>,
    /// The layout the workspace takes when it has this many containers or more, the highest
    /// threshold that is reached winning, in ascending order of threshold
    pub layout_rules: Vec<(usize, Layout)>,
    /// Threshold-based layout options rules (container_count >= threshold -> use these options).
    /// Sorted by threshold ascending at load time.
    #[serde(default)]
    pub layout_options_rules: Vec<(usize, LayoutOptions)>,
    /// Cached per-layout defaults from the global `layout_defaults` config setting.
    /// Pre-sorted at config load time; used as fallback when workspace has no overrides.
    #[serde(skip)]
    pub layout_defaults_cache: HashMap<DefaultLayout, CachedLayoutDefault>,
    pub layout_flip: Option<Axis>,
    pub workspace_padding: Option<i32>,
    pub container_padding: Option<i32>,
    pub latest_layout: Vec<Rect>,
    pub resize_dimensions: Vec<Option<Rect>>,
    pub tile: bool,
    /// The work area offset of this workspace, which it takes instead of its monitor's
    pub work_area_offset: Option<Rect>,
    /// The work area offset the workspace takes when it has this many containers or more, the
    /// highest threshold that is reached winning, in ascending order of threshold; it comes before
    /// [`Workspace::work_area_offset`]
    pub work_area_offset_rules: Vec<(usize, Rect)>,
    /// Whether this workspace takes its monitor's window-based offset (true by default)
    pub apply_window_based_work_area_offset: bool,
    /// What a window that opens on this workspace gets, when it is not the window manager's
    /// default: a container of its own, or a place in the focused container
    pub window_container_behaviour: Option<WindowContainerBehaviour>,
    /// What a window that opens on this workspace gets when the workspace has this many containers
    /// or more, the highest threshold that is reached winning, in ascending order of threshold;
    /// [`Workspace::update`] sets [`Workspace::window_container_behaviour`] from it
    pub window_container_behaviour_rules: Option<Vec<(usize, WindowContainerBehaviour)>>,
    /// Whether a window that opens on this workspace floats, when it is not the window manager's
    /// default
    pub float_override: Option<bool>,
    #[serde(skip)]
    pub globals: WorkspaceGlobals,
    /// Which of the workspace's two kinds of windows the focus commands act on
    pub layer: WorkspaceLayer,
    /// Whether a window that opens while this workspace is on its floating layer floats, when it
    /// is not the monitor's or the window manager's default
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
    /// The wallpaper this workspace shows instead of its monitor's
    pub wallpaper: Option<Wallpaper>,
    /// The entry of `komorebi.json` this workspace was loaded from, which keeps the rules that
    /// send windows to it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_config: Option<WorkspaceConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preselected_container_idx: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_swap_container_idx: Option<usize>,
    /// Initial window placement rules that determine where new tiled windows are placed
    /// ([`Workspace::new_container_for_window`]); with none, a window goes after the focused
    /// container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_window_placement_rules: Option<InitialWindowPlacementRules>,
    /// The strip of columns that the Scrolling layout lays containers out in, which is made when
    /// the layout first tiles the workspace ([`Workspace::layout_scrolling`])
    #[serde(skip)]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    pub scrolling: Option<ScrollingSpace<String>>,
    /// What only this platform keeps in a workspace (KDE: where the scrolling layout last placed the windows)
    #[serde(skip)]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    pub state: P::State,
}

impl<P: WorkspacePlatform> Default for Workspace<P> {
    fn default() -> Self {
        let (container_padding, workspace_padding) = P::default_padding();

        Self {
            name: None,
            containers: Default::default(),
            monocle_container: None,
            monocle_container_restore_idx: None,
            maximized_window: None,
            maximized_window_restore_idx: None,
            workspace_padding,
            container_padding,
            resize_dimensions: vec![],
            layout: Layout::Default(DefaultLayout::BSP),
            work_area_offset: None,
            work_area_offset_rules: vec![],
            latest_layout: vec![],
            layout_flip: None,
            layout_options: None,
            layout_rules: vec![],
            layout_options_rules: vec![],
            layout_defaults_cache: HashMap::new(),
            globals: Default::default(),
            layer: Default::default(),
            floating_layer_behaviour: None,
            tile: true,
            apply_window_based_work_area_offset: true,
            window_container_behaviour: None,
            window_container_behaviour_rules: None,
            floating_windows: Default::default(),
            float_override: None,
            wallpaper: None,
            workspace_config: None,
            preselected_container_idx: None,
            initial_window_placement_rules: None,
            promotion_swap_container_idx: None,
            scrolling: None,
            state: Default::default(),
        }
    }
}

/// Which of a workspace's two kinds of windows the focus, move and resize commands act on: the
/// tiles of its layout, or its floating windows
#[derive(Debug, Default, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum WorkspaceLayer {
    /// The containers of the layout
    #[default]
    Tiling,
    /// The floating windows
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

/// Where a workspace has a window of an application ([`Workspace::location_from_exe`])
#[derive(Debug)]
pub enum WorkspaceWindowLocation {
    Monocle(usize), // window_idx
    Maximized,
    Container(usize, usize), // container_idx, window_idx
    Floating(usize),         // idx in floating_windows
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Cached per-layout default options (pre-sorted rules) derived from the global `layout_defaults`.
pub struct CachedLayoutDefault {
    pub layout_options: Option<LayoutOptions>,
    /// Threshold-based rules, sorted by threshold ascending at load time
    pub layout_options_rules: Vec<(usize, LayoutOptions)>,
}

/// Convert an optional HashMap of threshold-based layout options rules into a Vec sorted by
/// threshold ascending.
pub fn sorted_layout_options_rules(
    rules: Option<&HashMap<usize, LayoutOptions>>,
) -> Vec<(usize, LayoutOptions)> {
    match rules {
        Some(rules) => {
            let mut sorted: Vec<(usize, LayoutOptions)> =
                rules.iter().map(|(t, o)| (*t, *o)).collect();
            sorted.sort_by_key(|(t, _)| *t);
            sorted
        }
        None => vec![],
    }
}

/// Find the highest matching threshold rule for the given container count.
/// Rules must be sorted by threshold ascending.
pub fn resolve_threshold_match(
    rules: &[(usize, LayoutOptions)],
    container_count: usize,
) -> Option<LayoutOptions> {
    rules
        .iter()
        .rev()
        .find(|(threshold, _)| container_count >= *threshold)
        .map(|(_, opts)| *opts)
}

impl<P: WorkspacePlatform> Workspace<P> {
    /// The containers of the workspace.
    pub const fn containers(&self) -> &std::collections::VecDeque<Container<P::Window>> {
        self.containers.elements()
    }

    /// The containers of the workspace, mutably.
    pub fn containers_mut(&mut self) -> &mut std::collections::VecDeque<Container<P::Window>> {
        self.containers.elements_mut()
    }

    /// The container that has the focus, if there is one.
    pub fn focused_container(&self) -> Option<&Container<P::Window>> {
        self.containers.focused()
    }

    /// The index of the container that has the focus.
    pub const fn focused_container_idx(&self) -> usize {
        self.containers.focused_idx()
    }

    /// The container that has the focus, mutably.
    pub fn focused_container_mut(&mut self) -> Option<&mut Container<P::Window>> {
        self.containers.focused_mut()
    }

    /// The floating windows of the workspace.
    pub const fn floating_windows(&self) -> &std::collections::VecDeque<P::Window> {
        self.floating_windows.elements()
    }

    /// The floating windows of the workspace, mutably.
    pub fn floating_windows_mut(&mut self) -> &mut std::collections::VecDeque<P::Window> {
        self.floating_windows.elements_mut()
    }

    /// The floating window that has the focus, if there is one.
    pub fn focused_floating_window(&self) -> Option<&P::Window> {
        self.floating_windows.focused()
    }

    /// The index of the floating window that has the focus.
    pub const fn focused_floating_window_idx(&self) -> usize {
        self.floating_windows.focused_idx()
    }

    /// The floating window that has the focus, mutably.
    pub fn focused_floating_window_mut(&mut self) -> Option<&mut P::Window> {
        self.floating_windows.focused_mut()
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container(&mut self, idx: usize) {
        tracing::info!("focusing container");

        self.containers.focus(idx);
    }

    pub fn swap_containers(&mut self, i: usize, j: usize) {
        self.containers.elements_mut().swap_respecting_locks(i, j);
        self.focus_container(j);
    }

    /// Reserves the index `insertion_idx` for the next window: an empty container is inserted
    /// there, which the layout gives a tile, and [`Workspace::new_container_for_window`] puts the
    /// next window in it
    pub fn preselect_container_idx(&mut self, insertion_idx: usize) {
        self.preselected_container_idx = Some(insertion_idx);
        self.insert_container_at_idx(insertion_idx, Container::preselect());
    }

    /// Removes the container a preselection reserved, if there is one
    pub fn cancel_preselect(&mut self) {
        if let Some(idx) = self.preselected_container_idx {
            self.containers_mut().remove_respecting_locks(idx);
            self.preselected_container_idx = None;
        }
    }

    /// Returns the index of the container whose tile is on the `direction` side of the focused
    /// one in the workspace's layout, or `None` when there is none on that side
    pub fn new_idx_for_direction(&self, direction: OperationDirection) -> Option<usize> {
        // in a strip, left and right are the neighbouring columns and up and down are the tiles of
        // the column, which the strip is the one to say
        if self.is_scrolling() && self.scrolling.is_some() {
            return P::scrolling_idx_for_direction(self, direction);
        }

        let len = NonZeroUsize::new(self.containers().len())?;

        direction.destination(
            self.layout.as_boxed_direction().as_ref(),
            self.layout_flip,
            self.focused_container_idx(),
            len,
            self.layout_options,
        )
    }

    /// Returns the index of the container after (or before) the focused one, wrapping around at
    /// the ends, or `None` when the workspace has no containers
    pub fn new_idx_for_cycle_direction(&self, direction: CycleDirection) -> Option<usize> {
        Option::from(direction.next_idx(
            self.focused_container_idx(),
            NonZeroUsize::new(self.containers().len())?,
        ))
    }

    pub fn container_idx_for_window(&self, window_id: PlatformWindowId<P>) -> Option<usize> {
        let mut idx = None;
        for (i, x) in self.containers().iter().enumerate() {
            if x.contains_window(window_id) {
                idx = Option::from(i);
            }
        }

        idx
    }

    /// Focuses the container the window is in, and the window in it. When the window is in a stack
    /// and was not the one in front, it is brought to the front ([`Container::load_focused_window`]).
    ///
    /// # Errors
    ///
    /// No container of the workspace has the window.
    pub fn focus_container_by_window(
        &mut self,
        window_id: PlatformWindowId<P>,
    ) -> eyre::Result<()> {
        let spot = P::hide_spot(self);

        let container_idx = self
            .container_idx_for_window(window_id)
            .ok_or_eyre("there is no container/window")?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_eyre("there is no container")?;

        let window_idx = container
            .idx_for_window(window_id)
            .ok_or_eyre("there is no window")?;

        let mut should_load = false;

        if container.focused_window_idx() != window_idx {
            should_load = true
        }

        container.focus_window(window_idx);

        if should_load {
            container.load_focused_window(spot)?;
        }

        self.focus_container(container_idx);

        Ok(())
    }

    /// Puts the window in a container of its own, and focuses that container: in the container a
    /// preselection reserved, if there is one, and otherwise at the index the workspace's initial
    /// window placement rules give it ([`Workspace::initial_window_placement_rules`]), which is
    /// after the focused container unless they say otherwise (at the start of an empty workspace)
    pub fn new_container_for_window(&mut self, window: P::Window) -> eyre::Result<()> {
        let spot = P::hide_spot(self);
        let next_idx = if let Some(idx) = self.preselected_container_idx {
            let next = idx;
            self.preselected_container_idx = None;
            self.remove_container_by_idx(next);
            next
        } else if self.containers().is_empty() {
            0
        } else if let Some(idx) = P::scrolling_insertion_idx(self) {
            idx
        } else {
            self.resolve_placement_index(&window)
        };

        let mut container = Container::default();
        container.add_window(window, spot)?;

        self.insert_container_at_idx(next_idx, container);

        Ok(())
    }

    /// Resolves the container index at which a new window should be placed,
    /// based on the `initial_window_placement_rules` configuration.
    ///
    /// Falls back to the default placement (`AfterFocused`,
    /// i.e. `focused_container_idx() + 1`) when:
    /// - No rules are configured
    /// - A `Rules` map has no matching rule for the window
    /// - A resolved index is out of bounds
    pub fn resolve_placement_index(&self, window: &P::Window) -> usize {
        let fallback_idx = self.focused_container_idx() + 1;

        let Some(rules) = &self.initial_window_placement_rules else {
            return fallback_idx;
        };

        match rules {
            InitialWindowPlacementRules::Target(target) => {
                self.resolve_placement_target(target, fallback_idx)
            }
            InitialWindowPlacementRules::Rules(rules_map) => {
                let (Some(title), Some(exe_name), Some(class), Some(path)) =
                    (window.title(), window.exe(), window.class(), window.path())
                else {
                    return fallback_idx;
                };
                // BTreeMap iterates in key order
                for (target, placement_rules) in rules_map {
                    let matched = match placement_rules {
                        PlacementMatchingRules::Single(id) => {
                            P::should_act_window_individual(&title, &exe_name, &class, &path, id)
                        }
                        PlacementMatchingRules::Many(rules) => {
                            // OR logic: any matching rule triggers placement at this target.
                            // Each MatchingRule handles its own Simple/Composite (AND) logic
                            // internally via should_act.
                            P::should_act_window(&title, &exe_name, &class, &path, rules).is_some()
                        }
                    };

                    if matched {
                        return self.resolve_placement_target(target, fallback_idx);
                    }
                }

                // No matching rule found
                fallback_idx
            }
        }
    }

    /// Resolves a `WindowPlacement` variant to a concrete container index.
    pub fn resolve_window_placement(
        &self,
        placement: &WindowPlacement,
        fallback_idx: usize,
    ) -> usize {
        match placement {
            WindowPlacement::AfterFocused => self.focused_container_idx() + 1,
            WindowPlacement::BeforeFocused => self.focused_container_idx(),
            WindowPlacement::Primary => {
                let idx = self.layout.primary_index();
                if idx <= self.containers().len() {
                    idx
                } else {
                    fallback_idx
                }
            }
            WindowPlacement::Secondary => {
                if let Some(idx) = self.layout.secondary_index(self.containers().len()) {
                    if idx <= self.containers().len() {
                        idx
                    } else {
                        fallback_idx
                    }
                } else {
                    fallback_idx
                }
            }
            WindowPlacement::Last => self.containers().len(),
        }
    }

    /// Resolves a `PlacementTarget` (either a named placement or a 1-based index) to a concrete container index.
    pub fn resolve_placement_target(&self, target: &PlacementTarget, fallback_idx: usize) -> usize {
        match target {
            PlacementTarget::Placement(placement) => {
                self.resolve_window_placement(placement, fallback_idx)
            }
            PlacementTarget::Index(idx) => {
                // Config indices are 1-based; convert to 0-based
                let zero_based = idx.saturating_sub(1);
                if zero_based <= self.containers().len() {
                    zero_based
                } else {
                    fallback_idx
                }
            }
        }
    }

    pub fn insert_container_at_idx(
        &mut self,
        idx: usize,
        container: Container<P::Window>,
    ) -> usize {
        let insertion_idx = self
            .containers_mut()
            .insert_respecting_locks(idx, container);

        if insertion_idx > self.resize_dimensions.len() {
            self.resize_dimensions.push(None);
        } else {
            self.resize_dimensions.insert(insertion_idx, None);
        }

        self.focus_container(insertion_idx);

        insertion_idx
    }

    pub fn remove_container_by_idx(&mut self, idx: usize) -> Option<Container<P::Window>> {
        // wherever it goes next, it goes as wide as it was
        P::note_scrolling_departure(self, idx);

        let container = self.containers_mut().remove_respecting_locks(idx);

        if idx < self.resize_dimensions.len() {
            self.resize_dimensions.remove(idx);
        }

        container
    }

    pub fn focus_previous_container(&mut self) {
        let focused_idx = self.focused_container_idx();
        self.focus_container(focused_idx.saturating_sub(1));
    }

    /// Returns true if the window that has the focus in the compositor is the maximized window or
    /// in the monocle container, and so out of the layout
    ///
    /// # Errors
    ///
    /// The compositor reports no focused window.
    pub fn is_focused_window_monocle_or_maximized(&self) -> eyre::Result<bool> {
        let window_id = P::foreground_window_id().ok_or_eyre("no foreground window")?;
        if let Some(window) = &self.maximized_window
            && window_id == window.id()
        {
            return Ok(true);
        }

        if let Some(container) = &self.monocle_container
            && container.contains_window(window_id)
        {
            return Ok(true);
        }

        Ok(false)
    }

    pub fn is_empty(&self) -> bool {
        self.containers().is_empty()
            && self.maximized_window.is_none()
            && self.monocle_container.is_none()
            && self.floating_windows().is_empty()
    }

    pub fn contains_window(&self, window_id: PlatformWindowId<P>) -> bool {
        for container in self.containers() {
            if container.contains_window(window_id) {
                return true;
            }
        }

        if let Some(window) = &self.maximized_window
            && window_id == window.id()
        {
            return true;
        }

        if let Some(container) = &self.monocle_container
            && container.contains_window(window_id)
        {
            return true;
        }

        for window in self.floating_windows() {
            if window_id == window.id() {
                return true;
            }
        }

        false
    }

    /// Where this workspace has a window of the executable `exe`: in a container first, then the
    /// maximized window, then the monocle container, then the floating windows
    pub fn location_from_exe(&self, exe: &str) -> Option<WorkspaceWindowLocation> {
        for (container_idx, container) in self.containers().iter().enumerate() {
            if let Some(window_idx) = container.idx_from_exe(exe) {
                return Some(WorkspaceWindowLocation::Container(
                    container_idx,
                    window_idx,
                ));
            }
        }

        if let Some(window) = &self.maximized_window
            && let Some(window_exe) = window.exe()
            && exe == window_exe
        {
            return Some(WorkspaceWindowLocation::Maximized);
        }

        if let Some(container) = &self.monocle_container
            && let Some(window_idx) = container.idx_from_exe(exe)
        {
            return Some(WorkspaceWindowLocation::Monocle(window_idx));
        }

        for (window_idx, window) in self.floating_windows().iter().enumerate() {
            if let Some(window_exe) = window.exe()
                && exe == window_exe
            {
                return Some(WorkspaceWindowLocation::Floating(window_idx));
            }
        }

        None
    }

    /// Returns true if and only if the window is in one of the containers, is the maximized
    /// window or is in the monocle container, which leaves out the floating windows: the windows
    /// komorebi tiles
    pub fn contains_managed_window(&self, window_id: PlatformWindowId<P>) -> bool {
        for container in self.containers() {
            if container.contains_window(window_id) {
                return true;
            }
        }

        if let Some(window) = &self.maximized_window
            && window_id == window.id()
        {
            return true;
        }

        if let Some(container) = &self.monocle_container
            && container.contains_window(window_id)
        {
            return true;
        }

        false
    }

    /// Moves the focused window of the focused container into the container at
    /// `target_container_idx`, which is how a window is stacked. The window is put in front of
    /// the target's, and the container it left is removed when it has no window left, or shows its
    /// next window.
    ///
    /// # Errors
    ///
    /// There is no focused container or window, or no container at the target's index (counted
    /// after the container the window left has been removed, if it goes).
    pub fn move_window_to_container(&mut self, target_container_idx: usize) -> eyre::Result<()> {
        let spot = P::hide_spot(self);
        let focused_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .ok_or_eyre("there is no container")?;

        let window = container
            .remove_focused_window()
            .ok_or_eyre("there is no window")?;

        // This is a little messy
        let adjusted_target_container_index = if container.windows().is_empty() {
            self.remove_container_by_idx(focused_idx);

            if focused_idx < target_container_idx {
                target_container_idx.saturating_sub(1)
            } else {
                target_container_idx
            }
        } else {
            container.load_focused_window(spot)?;
            target_container_idx
        };

        let target_container = self
            .containers_mut()
            .get_mut(adjusted_target_container_index)
            .ok_or_eyre("there is no container")?;

        target_container.add_window(window, spot)?;

        self.focus_container(adjusted_target_container_index);
        self.focused_container_mut()
            .ok_or_eyre("there is no container")?
            .load_focused_window(spot)?;

        Ok(())
    }

    /// Takes the focused window out of its container, which is how a window is unstacked, and puts
    /// it in a container of its own, where a new window goes ([`Workspace::new_container_for_window`]):
    /// after the focused one, unless the workspace's initial window placement rules say otherwise.
    /// The container it left is removed when it has no window left, or shows its next window.
    ///
    /// # Errors
    ///
    /// There is no focused container or window.
    pub fn new_container_for_focused_window(&mut self) -> eyre::Result<()> {
        let spot = P::hide_spot(self);
        let focused_container_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .ok_or_eyre("there is no container")?;

        let window = container
            .remove_focused_window()
            .ok_or_eyre("there is no window")?;

        if container.windows().is_empty() {
            self.remove_container_by_idx(focused_container_idx);
        } else {
            container.load_focused_window(spot)?;
        }

        self.new_container_for_window(window.clone())?;

        let mut container = Container::default();
        container.add_window(window, spot)?;
        Ok(())
    }

    pub fn remove_focused_container(&mut self) -> Option<Container<P::Window>> {
        let focused_idx = self.focused_container_idx();
        let container = self.remove_container_by_idx(focused_idx);
        self.focus_previous_container();

        container
    }

    pub fn remove_container(&mut self, idx: usize) -> Option<Container<P::Window>> {
        let container = self.remove_container_by_idx(idx);
        self.focus_previous_container();

        container
    }

    pub fn add_container_to_back(&mut self, container: Container<P::Window>) {
        self.containers_mut().push_back(container);
        self.focus_last_container();
    }

    pub fn add_container_to_front(&mut self, container: Container<P::Window>) {
        self.containers_mut().push_front(container);
        self.focus_first_container();
    }

    pub fn focus_last_container(&mut self) {
        self.focus_container(self.containers().len().saturating_sub(1));
    }

    pub fn focus_first_container(&mut self) {
        self.focus_container(0);
    }

    /// Takes the focused container out of the containers and makes it the monocle container,
    /// remembering its index, and focuses the container before it. The window in front of the
    /// monocle container is shown and the rest of its windows are hidden.
    ///
    /// The container's resize adjustment stays where it is, so that the container goes back to the
    /// size it had.
    ///
    /// # Errors
    ///
    /// The workspace has no containers.
    pub fn new_monocle_container(&mut self) -> eyre::Result<()> {
        let spot = P::hide_spot(self);
        let focused_idx = self.focused_container_idx();

        // we shouldn't use remove_container_by_idx here because it doesn't make sense for
        // monocle and maximized toggles which take over the whole screen before being reinserted
        // at the same index to respect locked container indexes
        let container = self
            .containers_mut()
            .remove(focused_idx)
            .ok_or_eyre("there is no container")?;

        // We don't remove any resize adjustments for a monocle, because when this container is
        // inevitably reintegrated, it would be weird if it doesn't go back to the dimensions
        // it had before

        self.monocle_container = Option::from(container);
        self.monocle_container_restore_idx = Option::from(focused_idx);
        self.focus_previous_container();

        self.monocle_container
            .as_mut()
            .ok_or_eyre("there is no monocle container")?
            .load_focused_window(spot)?;

        Ok(())
    }

    /// Puts the monocle container back and makes the next (or the previous) container the
    /// monocle, wrapping around at the ends
    ///
    /// A workspace with no other container is left as it is.
    ///
    /// # Errors
    ///
    /// The workspace has no monocle container.
    pub fn cycle_monocle_container(&mut self, direction: CycleDirection) -> eyre::Result<()> {
        if self.containers().is_empty() {
            return Ok(());
        }

        self.reintegrate_monocle_container()?;

        let new_idx = self
            .new_idx_for_cycle_direction(direction)
            .ok_or_eyre("there is no container to cycle monocle to")?;

        self.focus_container(new_idx);
        self.new_monocle_container()?;

        Ok(())
    }

    /// Puts the monocle container back in the containers at the index it was taken from, and
    /// focuses it. The window in front of the container is shown and the rest are hidden.
    ///
    /// # Errors
    ///
    /// The workspace has no monocle container.
    pub fn reintegrate_monocle_container(&mut self) -> eyre::Result<()> {
        let spot = P::hide_spot(self);
        let restore_idx = self
            .monocle_container_restore_idx
            .ok_or_eyre("there is no monocle restore index")?;

        let container = self
            .monocle_container
            .as_ref()
            .ok_or_eyre("there is no monocle container")?;

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
            .ok_or_eyre("there is no container")?
            .load_focused_window(spot)?;

        self.monocle_container = None;
        self.monocle_container_restore_idx = None;

        Ok(())
    }

    /// Puts the floating window that has the focus in the compositor back in the layout, in a
    /// container of its own at the index of the focused container
    ///
    /// # Errors
    ///
    /// The window the compositor has focused is not a floating window of the workspace.
    pub fn new_container_for_floating_window(&mut self) -> eyre::Result<()> {
        let spot = P::hide_spot(self);
        let focused_idx = self.focused_container_idx();
        let window = self
            .remove_focused_floating_window()
            .ok_or_eyre("there is no floating window")?;

        let mut container = Container::default();
        container.add_window(window, spot)?;

        self.insert_container_at_idx(focused_idx, container);

        Ok(())
    }

    pub fn remove_focused_floating_window(&mut self) -> Option<P::Window> {
        let window_id = P::foreground_window_id()?;
        self.remove_floating_window(window_id)
    }

    pub fn remove_floating_window(&mut self, window_id: PlatformWindowId<P>) -> Option<P::Window> {
        let mut idx = None;
        for (i, window) in self.floating_windows().iter().enumerate() {
            if window_id == window.id() {
                idx = Option::from(i);
            }
        }

        match idx {
            None => None,
            Some(idx) => {
                if self.floating_windows().get(idx).is_some() {
                    P::before_floating_removed(self);
                    self.floating_windows_mut().remove(idx)
                } else {
                    None
                }
            }
        }
    }

    pub fn focus_previous_floating_window(&mut self) {
        let focused_idx = self.focused_floating_window_idx();
        self.focus_floating_window(focused_idx.saturating_sub(1));
    }

    pub fn focus_floating_window(&mut self, idx: usize) {
        tracing::info!("focusing floating window");

        self.floating_windows.focus(idx);
    }

    /// Returns the index of the container whose tile has the pointer in it, or `None` when the
    /// pointer is on no tile, or the backend cannot say where it is
    ///
    /// The tiles are those the layout last calculated ([`Workspace::latest_layout`]).
    pub fn container_idx_from_current_point(&self) -> Option<usize> {
        let mut idx = None;

        let point = P::cursor_position()?;

        for (i, _container) in self.containers().iter().enumerate() {
            if let Some(rect) = self.latest_layout.get(i)
                && rect.contains_point(point)
            {
                idx = Option::from(i);
            }
        }

        idx
    }

    /// The container that has the window, if one of this workspace's does
    pub fn container_for_window(
        &self,
        window_id: PlatformWindowId<P>,
    ) -> Option<&Container<P::Window>> {
        self.containers()
            .get(self.container_idx_for_window(window_id)?)
    }

    /// The windows that are in view on this workspace: the maximized window (an entry that is
    /// `None` when there is none), the one at the top of the monocle container, the one at the top
    /// of each container, and every floating window
    pub fn visible_windows(&self) -> Vec<Option<&P::Window>> {
        let mut vec = vec![];

        vec.push(self.maximized_window.as_ref());

        if let Some(monocle) = &self.monocle_container {
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

    pub fn enforce_resize_constraints(&mut self) {
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
            _ => self.enforce_no_resize(),
        }
    }

    pub fn enforce_resize_constraints_for_bsp(&mut self) {
        for (i, rect) in self.resize_dimensions.iter_mut().enumerate() {
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
        if let Some(Some(first)) = self.resize_dimensions.first_mut() {
            first.top = 0;
            first.left = 0;
        }

        // The last container can never be resized to the bottom or the right
        if let Some(Some(last)) = self.resize_dimensions.last_mut() {
            last.bottom = 0;
            last.right = 0;
        }
    }

    pub fn enforce_resize_for_columns(&mut self) {
        let resize_dimensions = &mut self.resize_dimensions;
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

    pub fn enforce_resize_for_rows(&mut self) {
        let resize_dimensions = &mut self.resize_dimensions;
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

    pub fn enforce_resize_for_vertical_stack(&mut self) {
        let resize_dimensions = &mut self.resize_dimensions;
        match resize_dimensions.len() {
            // Single window can not be resized at all
            0 | 1 => self.enforce_no_resize(),
            _ => {
                // Zero is actually on the left
                if let Some(left) = resize_dimensions[0].as_mut() {
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

    pub fn enforce_resize_for_right_vertical_stack(&mut self) {
        let resize_dimensions = &mut self.resize_dimensions;
        match resize_dimensions.len() {
            // Single window can not be resized at all
            0 | 1 => self.enforce_no_resize(),
            _ => {
                // Zero is actually on the right
                if let Some(right) = resize_dimensions[0].as_mut() {
                    right.top = 0;
                    right.bottom = 0;
                    right.right = 0;
                }

                // Handle stack on the left
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

    pub fn enforce_resize_for_horizontal_stack(&mut self) {
        let resize_dimensions = &mut self.resize_dimensions;
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                if let Some(left) = resize_dimensions[0].as_mut() {
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

    pub fn enforce_resize_for_ultrawide(&mut self) {
        let resize_dimensions = &mut self.resize_dimensions;
        match resize_dimensions.len() {
            // Single window can not be resized at all
            0 | 1 => self.enforce_no_resize(),
            // Two windows can only be resized in the middle
            2 => {
                // Zero is actually on the right
                if let Some(right) = resize_dimensions[0].as_mut() {
                    right.top = 0;
                    right.bottom = 0;
                    right.right = 0;
                }

                // One is on the left
                if let Some(left) = resize_dimensions[1].as_mut() {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }
            }
            // Three or more windows means 0 is in center, 1 is at the left, 2.. are a vertical
            // stack on the right
            _ => {
                // Central can be resized left or right
                if let Some(right) = resize_dimensions[0].as_mut() {
                    right.top = 0;
                    right.bottom = 0;
                }

                // Left one can only be resized to the right
                if let Some(left) = resize_dimensions[1].as_mut() {
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

    pub fn enforce_no_resize(&mut self) {
        for rect in self.resize_dimensions.iter_mut().flatten() {
            rect.left = 0;
            rect.right = 0;
            rect.top = 0;
            rect.bottom = 0;
        }
    }

    /// Whether the workspace is laid out as a strip of columns.
    pub fn is_scrolling(&self) -> bool {
        matches!(self.layout, Layout::Default(DefaultLayout::Scrolling))
    }
    /// Moves the focused container to the primary position of the layout and focuses it.
    pub fn promote_container(&mut self) -> eyre::Result<()> {
        let focused_idx = self.focused_container_idx();
        let container = self
            .containers_mut()
            .remove_respecting_locks(focused_idx)
            .ok_or_eyre("there is no container")?;

        let primary_idx = P::primary_container_idx(&self.layout)?;

        let insertion_idx = self
            .containers_mut()
            .insert_respecting_locks(primary_idx, container);
        self.focus_container(insertion_idx);

        Ok(())
    }

    /// The layout options in force for the number of containers the workspace has: its own, or
    /// those its layout has by default.
    pub fn effective_layout_options(&self) -> Option<LayoutOptions> {
        let container_count = self.containers().len();

        let has_workspace_overrides =
            self.layout_options.is_some() || !self.layout_options_rules.is_empty();

        let (effective_base, effective_rules): (Option<LayoutOptions>, &[(usize, LayoutOptions)]) =
            if has_workspace_overrides {
                (self.layout_options, &self.layout_options_rules)
            } else {
                match &self.layout {
                    Layout::Default(dl) => match self.layout_defaults_cache.get(dl) {
                        Some(entry) => (entry.layout_options, &entry.layout_options_rules),
                        None => (None, &[]),
                    },
                    // a layout that is not one of the defaults (the Windows custom layouts) has none
                    #[allow(unreachable_patterns)]
                    _ => (None, &[]),
                }
            };

        resolve_threshold_match(effective_rules, container_count).or(effective_base)
    }

    /// Takes the focused window of the workspace (the maximized one, the monocle's or the focused
    /// container's) out of its place and makes it a floating window.
    pub fn new_floating_window(&mut self) -> eyre::Result<()> {
        let spot = P::hide_spot(self);
        let window = if let Some(maximized_window) = self.maximized_window.clone() {
            P::unmaximize_window(&maximized_window)?;
            self.maximized_window = None;
            self.maximized_window_restore_idx = None;
            maximized_window
        } else if let Some(monocle_container) = &mut self.monocle_container {
            let window = monocle_container
                .remove_focused_window()
                .ok_or_eyre("there is no window")?;

            if monocle_container.windows().is_empty() {
                self.monocle_container = None;
                self.monocle_container_restore_idx = None;
            } else {
                monocle_container.load_focused_window(spot)?;
            }

            window
        } else {
            let focused_idx = self.focused_container_idx();

            let container = self
                .focused_container_mut()
                .ok_or_eyre("there is no container")?;

            let window = container
                .remove_focused_window()
                .ok_or_eyre("there is no window")?;

            if container.windows().is_empty() {
                self.remove_container_by_idx(focused_idx);

                if focused_idx == self.containers().len() {
                    self.focus_container(focused_idx.saturating_sub(1));
                }
            } else {
                container.load_focused_window(spot)?;
            }

            window
        };

        self.floating_windows_mut().push_back(window);

        Ok(())
    }

    /// Hides every window of the workspace, or every one but `omit`: the floating windows and the
    /// windows of every container, the maximized window and the monocle container.
    pub fn hide(&mut self, omit: Option<PlatformWindowId<P>>) -> eyre::Result<()> {
        let spot = P::hide_spot(self);
        let display_spot = P::hide_spot_on_display(self);

        for window in self.floating_windows_mut().iter_mut().rev() {
            let mut should_hide = omit.is_none();

            if !should_hide
                && let Some(omit) = omit
                && omit != window.id()
            {
                should_hide = true
            }

            if should_hide && let Err(error) = window.hide_window(spot) {
                tracing::warn!(
                    "failed to hide floating window, but this might be because it is already hidden: {error}"
                );
            }
        }

        let resize_dimensions = P::before_hide_containers(self);

        for container in self.containers_mut() {
            container.hide(omit, display_spot)?;
        }

        if let Some(resize_dimensions) = resize_dimensions {
            self.resize_dimensions = resize_dimensions;
        }

        if let Some(window) = &self.maximized_window {
            window.hide_window(spot)?;
        }

        if let Some(container) = &mut self.monocle_container {
            container.hide(omit, display_spot)?;
        }

        Ok(())
    }

    /// Lays the windows of the workspace out: asks the layout for a tile per container in the
    /// work area (less the paddings, the offsets and the room a border takes) and has the platform
    /// put the windows on their tiles. What is the platform's (animations, title bars, stackbars,
    /// the clamp to the work area, the scrolling layout's extra pass) is in the hooks of
    /// [`WorkspacePlatform`] that `update_shared` calls, in the order of its documentation.
    ///
    /// Not named `update`, so that a daemon keeps its own `update` (it calls this one).
    pub fn update_shared(&mut self) -> eyre::Result<()> {
        // the platform may skip the update, and may hold a guard that lives until it is done
        let _guard = match P::begin_update(self)? {
            Flow::Skip => return Ok(()),
            Flow::Run(guard) => guard,
        };

        // make sure we are never holding on to empty containers
        let focused_container_idx = self.focused_container_idx();
        self.containers_mut()
            .retain(|c| c.is_preselect() || !c.windows().is_empty());
        P::containers_pruned(self, focused_container_idx);

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

        let managed_maximized_window = self.maximized_window.is_some();

        if self.tile {
            if let Some(container) = &mut self.monocle_container {
                if let Some(window) = container.focused_window_mut() {
                    adjusted_work_area.add_padding(container_padding);
                    adjusted_work_area.add_padding(border_offset);
                    adjusted_work_area.add_padding(border_width);
                    P::place_monocle(window, &adjusted_work_area)?;
                };
            } else if let Some(window) = &mut self.maximized_window {
                P::maximize_in_layout(window)?;
            } else if !self.containers().is_empty() {
                let is_scrolling = self.is_scrolling();
                let around_window = container_padding + border_offset + border_width;

                // windows that were not what they were told to be since the last layout
                if is_scrolling && P::adopt_widths_before_layout(self) {
                    P::adopt_actual_widths(self, around_window, adjusted_work_area.right);
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

                    // the containers that rest where another monitor is seen from, which are hidden
                    let mut hidden = vec![];
                    let mut layouts = if is_scrolling {
                        let (layouts, hide) =
                            P::scrolling_layouts(self, &adjusted_work_area, container_padding);
                        hidden = hide;
                        layouts
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

                    let pass_state = P::begin_pass(self);

                    let placement = Placement {
                        adjusted_work_area,
                        work_area,
                        container_padding,
                        border_offset,
                        border_width,
                        is_scrolling,
                        resize_dimensions_is_empty: self.resize_dimensions.is_empty(),
                        managed_maximized_window,
                        pass: pass_state.as_deref(),
                    };

                    for (i, container) in self.containers_mut().iter_mut().enumerate() {
                        if let Some(layout) = layouts.get_mut(i) {
                            P::adjust_layout(container, layout, &placement);
                            P::place_container(
                                container,
                                layout,
                                hidden.get(i).copied().unwrap_or_default(),
                                &placement,
                            )?;
                        }
                    }

                    self.latest_layout = layouts;

                    match P::end_update(self, pass, around_window, adjusted_work_area.right)? {
                        Pass::Again => {
                            pass += 1;
                            continue;
                        }
                        Pass::Done => break,
                    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    struct W(u32);

    thread_local! {
        static LOG: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn log() -> Vec<String> {
        LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
    }

    thread_local! {
        static HIDDEN: std::cell::RefCell<Vec<(u32, u8)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// the spot that `Plat` asks windows to be hidden at, and the one for the containers of a
    /// workspace that is being hidden
    const SPOT: u8 = 7;
    const DISPLAY_SPOT: u8 = 9;

    fn hidden() -> Vec<(u32, u8)> {
        HIDDEN.with(|hidden| std::mem::take(&mut *hidden.borrow_mut()))
    }

    impl ContainerWindow for W {
        type Id = u32;
        type HideSpot = u8;

        fn id(&self) -> u32 {
            self.0
        }

        fn exe_name(&self) -> Option<String> {
            Some(format!("exe{}", self.0))
        }

        fn hide_window(&self, spot: u8) -> eyre::Result<()> {
            HIDDEN.with(|hidden| hidden.borrow_mut().push((self.0, spot)));
            Ok(())
        }

        fn restore_window(&self) -> eyre::Result<()> {
            Ok(())
        }

        fn hide_stacked(&self, spot: u8) -> eyre::Result<()> {
            HIDDEN.with(|hidden| hidden.borrow_mut().push((self.0, spot)));
            Ok(())
        }

        fn restore_stacked(&self) -> eyre::Result<()> {
            Ok(())
        }
    }

    impl WorkspaceWindow for W {
        fn title(&self) -> Option<String> {
            Some("title".to_string())
        }

        fn exe(&self) -> Option<String> {
            self.exe_name()
        }

        fn class(&self) -> Option<String> {
            Some("class".to_string())
        }

        fn path(&self) -> Option<String> {
            None
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Plat;

    impl WorkspacePlatform for Plat {
        type Window = W;
        type State = ();

        fn hide_spot(_workspace: &Workspace<Self>) -> u8 {
            SPOT
        }

        fn hide_spot_on_display(_workspace: &Workspace<Self>) -> u8 {
            DISPLAY_SPOT
        }

        fn foreground_window_id() -> Option<u32> {
            Some(2)
        }

        fn cursor_position() -> Option<(i32, i32)> {
            None
        }

        fn should_act_window(
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &[MatchingRule],
        ) -> Option<MatchingRule> {
            None
        }

        fn should_act_window_individual(
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &IdWithIdentifier,
        ) -> bool {
            false
        }

        fn scrolling_idx_for_direction(
            _: &Workspace<Self>,
            _: OperationDirection,
        ) -> Option<usize> {
            None
        }

        fn scrolling_insertion_idx(_: &Workspace<Self>) -> Option<usize> {
            None
        }

        fn note_scrolling_departure(_: &Workspace<Self>, _: usize) {}

        fn before_floating_removed(workspace: &mut Workspace<Self>) {
            LOG.with(|log| {
                log.borrow_mut()
                    .push(format!("floating {}", workspace.floating_windows().len()))
            });
        }

        fn begin_update(_: &mut Workspace<Self>) -> eyre::Result<Flow> {
            LOG.with(|log| log.borrow_mut().push("begin".to_string()));
            Ok(Flow::Run(None))
        }

        fn begin_pass(_: &Workspace<Self>) -> Option<Box<dyn std::any::Any>> {
            LOG.with(|log| log.borrow_mut().push("begin_pass".to_string()));
            Some(Box::new(7u8))
        }

        fn containers_pruned(_: &mut Workspace<Self>, focused_before: usize) {
            LOG.with(|log| log.borrow_mut().push(format!("pruned {focused_before}")));
        }

        fn adjust_layout(_: &mut Container<W>, layout: &mut Rect, _: &Placement) {
            layout.add_padding(1);
        }

        fn place_container(
            container: &mut Container<W>,
            layout: &Rect,
            hidden: bool,
            placement: &Placement,
        ) -> eyre::Result<()> {
            LOG.with(|log| {
                log.borrow_mut().push(format!(
                    "place {} {} {} {:?}",
                    container.windows()[0].0,
                    layout.left,
                    hidden,
                    placement.pass.and_then(|pass| pass.downcast_ref::<u8>())
                ))
            });
            Ok(())
        }

        fn end_update(_: &mut Workspace<Self>, pass: usize, _: i32, _: i32) -> eyre::Result<Pass> {
            LOG.with(|log| log.borrow_mut().push(format!("end {pass}")));
            Ok(if pass == 0 { Pass::Again } else { Pass::Done })
        }
    }

    fn workspace_of(ids: &[u32]) -> Workspace<Plat> {
        let mut workspace = Workspace::<Plat>::default();
        for id in ids {
            workspace.new_container_for_window(W(*id)).unwrap();
        }
        workspace
    }

    #[test]
    fn a_new_window_gets_a_container_after_the_focused_one() {
        let workspace = workspace_of(&[1, 2, 3]);

        assert_eq!(workspace.containers().len(), 3);
        assert!(workspace.contains_window(2));
        assert_eq!(workspace.container_idx_for_window(3), Some(2));
        assert!(!workspace.contains_window(9));
    }

    #[test]
    fn the_focused_window_is_the_one_the_platform_says() {
        let mut workspace = workspace_of(&[1, 2]);

        assert_eq!(workspace.focused_container_idx(), 1);
        workspace.focus_container(0);
        assert_eq!(workspace.focused_container_idx(), 0);
        assert!(workspace.is_focused_window_monocle_or_maximized().is_ok());
    }

    #[test]
    fn the_json_of_a_workspace_names_the_same_fields_whatever_the_platform() {
        let json = serde_json::to_value(workspace_of(&[1])).unwrap();
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();

        assert!(keys.contains(&"containers"));
        assert!(keys.contains(&"floating_windows"));
        assert!(!keys.contains(&"globals"));
        assert!(!keys.contains(&"state"));
        assert!(!keys.contains(&"scrolling"));
    }

    #[test]
    fn the_shared_update_calls_the_hooks_in_order_and_runs_again_when_asked() {
        let mut workspace = workspace_of(&[1, 2]);
        workspace.globals.work_area = Rect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };
        log();

        workspace.update_shared().unwrap();

        let log = log();
        assert_eq!(log[0], "begin");
        assert_eq!(log[1], "pruned 1");
        assert_eq!(log.iter().filter(|l| l.starts_with("place ")).count(), 4);
        assert_eq!(log[2], "begin_pass");
        assert_eq!(log[3], "place 1 1 false Some(7)");
        assert_eq!(log[5], "end 0");
        assert_eq!(log[6], "begin_pass");
        assert_eq!(log[9], "end 1");
        assert_eq!(workspace.latest_layout.len(), 2);
        assert_eq!(workspace.resize_dimensions.len(), 2);
    }

    #[test]
    fn the_hook_for_a_removed_floating_window_runs_before_the_removal() {
        let mut workspace = Workspace::<Plat>::default();
        workspace.floating_windows_mut().push_back(W(1));
        workspace.floating_windows_mut().push_back(W(2));
        log();

        assert_eq!(workspace.remove_floating_window(2), Some(W(2)));

        // the ring still had both windows when the hook ran: the focus moves first
        assert_eq!(log(), ["floating 2"]);
        assert_eq!(workspace.floating_windows().len(), 1);
    }

    #[test]
    fn promoting_a_container_puts_it_first_and_focuses_it() {
        let mut workspace = workspace_of(&[1, 2, 3]);

        workspace.promote_container().unwrap();

        assert_eq!(workspace.focused_container_idx(), 0);
        assert_eq!(workspace.container_idx_for_window(3), Some(0));
    }

    #[test]
    fn a_focused_window_becomes_floating_and_its_empty_container_goes() {
        let mut workspace = workspace_of(&[1, 2]);

        workspace.new_floating_window().unwrap();

        assert_eq!(workspace.containers().len(), 1);
        assert_eq!(workspace.floating_windows().len(), 1);
        assert!(workspace.contains_window(2));
    }

    #[test]
    fn hiding_leaves_out_the_window_it_is_told_to() {
        let mut workspace = workspace_of(&[1, 2, 3]);
        workspace.floating_windows_mut().push_back(W(9));
        hidden();

        workspace.hide(Some(2)).unwrap();

        let mut hidden = hidden();
        hidden.sort();
        // the floating window at the spot of a window, the containers at the one for a display
        assert_eq!(hidden, [(1, DISPLAY_SPOT), (3, DISPLAY_SPOT), (9, SPOT)]);
    }

    #[test]
    fn the_functions_that_stack_and_unstack_hide_at_the_spot_of_the_platform() {
        let mut workspace = workspace_of(&[1, 2]);
        hidden();

        // stacking window 2 on container 0 hides window 1, which was in front, at the spot
        workspace.focus_container(1);
        workspace.move_window_to_container(0).unwrap();

        // once by add_window, once by load_focused_window, both at the platform's spot
        assert_eq!(hidden(), [(1, SPOT), (1, SPOT)]);
    }

    #[test]
    fn a_workspace_over_a_unit_state_is_a_workspace_with_no_padding_by_default() {
        let workspace = Workspace::<Plat>::default();

        assert_eq!(workspace.container_padding, None);
        assert!(workspace.tile);
        assert_eq!(workspace.layout, Layout::Default(DefaultLayout::BSP));
    }

    #[test]
    fn promote_focus_falls_back_to_the_primary_position_and_then_to_zero() {
        assert_eq!(
            Plat::promote_focus_container_idx(&Layout::Default(DefaultLayout::BSP)),
            0
        );
    }
}
