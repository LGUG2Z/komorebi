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
use crate::core::Axis;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::FloatingLayerBehaviour;
use crate::core::InitialWindowPlacementRules;
use crate::core::Layout;
use crate::core::LayoutDefaultEntry;
use crate::core::LayoutOptions;
use crate::core::OperationDirection;
use crate::core::PlacementMatchingRules;
use crate::core::PlacementTarget;
use crate::core::Rect;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowPlacement;
use crate::lockable_sequence::LockableSequence;
use crate::ring::Ring;
use crate::static_config::Wallpaper;
use crate::static_config::WorkspaceConfig;
use crate::theme_manager;
use crate::window::Window;
use crate::window::WindowDetails;
use crate::window::should_act;
use crate::window::should_act_individual;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_backend::MonitorId;
use komorebi_backend::WindowId;
use komorebi_scrolling::ScrollingSpace;
use komorebi_themes::KomorebiTheme;
use komorebi_themes::KomorebiThemeCustom;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::num::NonZeroUsize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Workspace {
    pub name: Option<String>,
    pub containers: Ring<Container>,
    /// The container that has the whole work area, and so is the only one in view: it is not one
    /// of `containers`, which are not given tiles until it is put back
    /// ([`Workspace::new_monocle_container`], [`Workspace::reintegrate_monocle_container`])
    pub monocle_container: Option<Container>,
    /// The index in `containers` the monocle container goes back to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monocle_container_restore_idx: Option<usize>,
    /// The window the compositor has maximized, which is in no container: the containers are left
    /// where they are, behind it ([`Workspace::new_maximized_window`],
    /// [`Workspace::reintegrate_maximized_window`])
    pub maximized_window: Option<Window>,
    /// The index in `containers` the maximized window goes back to, in a container of its own
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximized_window_restore_idx: Option<usize>,
    pub floating_windows: Ring<Window>,
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
    pub(crate) layout_defaults_cache: HashMap<DefaultLayout, CachedLayoutDefault>,
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
    pub(crate) scrolling: Option<ScrollingSpace<String>>,
    /// When the windows of a Scrolling workspace were last sent to where the strip puts them, and
    /// how wide each container was made to be, for telling when the sizes that they report can be
    /// believed ([`crate::scrolling::WIDTH_SETTLE`])
    #[serde(skip)]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    pub(crate) scrolling_placed: Option<crate::scrolling::Placed>,
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

impl_ring_elements!(Workspace, Container);
impl_ring_elements!(Workspace, Window, "floating_window");

impl Default for Workspace {
    fn default() -> Self {
        Self {
            name: None,
            containers: Default::default(),
            monocle_container: None,
            monocle_container_restore_idx: None,
            maximized_window: None,
            maximized_window_restore_idx: None,
            workspace_padding: None,
            container_padding: None,
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
            scrolling_placed: None,
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

#[derive(Debug, Default, Copy, Clone, Serialize, Deserialize, PartialEq)]
/// The work area a monitor gives the workspace it shows, the padding and the offset the
/// workspace falls back to when it sets none of its own, the monitor's window-based offset, and
/// the room around each window that a border drawn by the compositor takes
pub struct WorkspaceGlobals {
    pub container_padding: Option<i32>,
    pub workspace_padding: Option<i32>,
    pub border_width: i32,
    pub border_offset: i32,
    pub work_area: Rect,
    pub work_area_offset: Option<Rect>,
    pub window_based_work_area_offset: Option<Rect>,
    pub window_based_work_area_offset_limit: isize,
    /// The floating layer behaviour of the monitor, which the workspace's own overrides
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Cached per-layout default options (pre-sorted rules) derived from the global `layout_defaults`.
pub(crate) struct CachedLayoutDefault {
    pub layout_options: Option<LayoutOptions>,
    /// Threshold-based rules, sorted by threshold ascending at load time
    pub layout_options_rules: Vec<(usize, LayoutOptions)>,
}

/// Convert an optional HashMap of threshold-based layout options rules into a Vec sorted by
/// threshold ascending.
fn sorted_layout_options_rules(
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
fn resolve_threshold_match(
    rules: &[(usize, LayoutOptions)],
    container_count: usize,
) -> Option<LayoutOptions> {
    rules
        .iter()
        .rev()
        .find(|(threshold, _)| container_count >= *threshold)
        .map(|(_, opts)| *opts)
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

impl Workspace {
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
    pub fn load_static_config(
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

    /// Compute effective layout options using the complete-replacement cascade:
    ///
    /// If the workspace defines EITHER `layout_options` OR `layout_options_rules`,
    /// it completely replaces the global `layout_defaults` for this layout.
    /// Global defaults are only used when the workspace has NEITHER setting.
    ///
    /// Within the effective source (workspace or global):
    ///   1. Try threshold match from rules (highest matching threshold wins)
    ///   2. If a rule matches -> use it (full replacement of base)
    ///   3. Else -> use the base `layout_options`
    pub(crate) fn effective_layout_options(&self) -> Option<LayoutOptions> {
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
                }
            };

        resolve_threshold_match(effective_rules, container_count).or(effective_base)
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
            return self.scrolling_idx_for_direction(direction);
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

    pub fn container_idx_for_window(&self, window_id: u32) -> Option<usize> {
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
    pub fn focus_container_by_window(&mut self, window_id: u32) -> eyre::Result<()> {
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
            container.load_focused_window();
        }

        self.focus_container(container_idx);

        Ok(())
    }

    /// Puts the window in a container of its own, and focuses that container: in the container a
    /// preselection reserved, if there is one, and otherwise at the index the workspace's initial
    /// window placement rules give it ([`Workspace::initial_window_placement_rules`]), which is
    /// after the focused container unless they say otherwise (at the start of an empty workspace)
    pub fn new_container_for_window(&mut self, window: Window) {
        let next_idx = if let Some(idx) = self.preselected_container_idx {
            let next = idx;
            self.preselected_container_idx = None;
            self.remove_container_by_idx(next);
            next
        } else if self.containers().is_empty() {
            0
        } else if let Some(idx) = self.scrolling_insertion_idx() {
            idx
        } else {
            self.resolve_placement_index(&window)
        };

        let mut container = Container::default();
        container.add_window(window);

        self.insert_container_at_idx(next_idx, container);
    }

    /// Resolves the container index at which a new window should be placed,
    /// based on the `initial_window_placement_rules` configuration.
    ///
    /// Falls back to the default placement (`AfterFocused`,
    /// i.e. `focused_container_idx() + 1`) when:
    /// - No rules are configured
    /// - A `Rules` map has no matching rule for the window
    /// - A resolved index is out of bounds
    fn resolve_placement_index(&self, window: &Window) -> usize {
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
                let path = path.to_string_lossy();

                let regex_identifiers = REGEX_IDENTIFIERS.lock().clone();

                // BTreeMap iterates in key order
                for (target, placement_rules) in rules_map {
                    let matched = match placement_rules {
                        PlacementMatchingRules::Single(id) => should_act_individual(
                            &title,
                            &exe_name,
                            &[class.as_str()],
                            &path,
                            id,
                            &regex_identifiers,
                        ),
                        PlacementMatchingRules::Many(rules) => {
                            // OR logic: any matching rule triggers placement at this target.
                            // Each MatchingRule handles its own Simple/Composite (AND) logic
                            // internally via should_act.
                            should_act(
                                &title,
                                &exe_name,
                                &[class.as_str()],
                                &path,
                                rules,
                                &regex_identifiers,
                            )
                            .is_some()
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
    fn resolve_window_placement(&self, placement: &WindowPlacement, fallback_idx: usize) -> usize {
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
    fn resolve_placement_target(&self, target: &PlacementTarget, fallback_idx: usize) -> usize {
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

    // this fn respects locked container indexes - we should use it for pretty much everything
    // except monocle and maximize toggles
    pub fn insert_container_at_idx(&mut self, idx: usize, container: Container) -> usize {
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

    pub fn prune_duplicate_windows(&mut self) -> eyre::Result<()> {
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

    /// Remove every window that no longer exists
    #[tracing::instrument(skip(self))]
    pub fn reap_invalid_windows(&mut self) -> eyre::Result<usize> {
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

    /// Takes the window out of its container, and returns it. A container that is left with no
    /// window is removed. One that is left with windows shows the one in front of them now and
    /// gives it the focus in the compositor, so that closing the window in front of a stack brings
    /// the next one up.
    ///
    /// # Errors
    ///
    /// No container of the workspace has the window.
    pub fn remove_window(&mut self, window_id: u32) -> eyre::Result<Window> {
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
                c.restore();
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
            container.load_focused_window();
            if let Some(window) = container.focused_window() {
                window.focus(false)?;
            }
        }

        Ok(window)
    }

    // this fn respects locked container indexes - we should use it for pretty much everything
    // except monocle and maximize toggles
    pub fn remove_container_by_idx(&mut self, idx: usize) -> Option<Container> {
        // wherever it goes next, it goes as wide as it was
        self.note_scrolling_departure(idx);

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
        let window_id = backend()
            .foreground_window()
            .and_then(WindowId::to_u32)
            .ok_or_eyre("no foreground window")?;
        if let Some(window) = self.maximized_window
            && window_id == window.id
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

    pub fn contains_window(&self, window_id: u32) -> bool {
        for container in self.containers() {
            if container.contains_window(window_id) {
                return true;
            }
        }

        if let Some(window) = self.maximized_window
            && window_id == window.id
        {
            return true;
        }

        if let Some(container) = &self.monocle_container
            && container.contains_window(window_id)
        {
            return true;
        }

        for window in self.floating_windows() {
            if window_id == window.id {
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

        if let Some(window) = self.maximized_window
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
    pub fn contains_managed_window(&self, window_id: u32) -> bool {
        for container in self.containers() {
            if container.contains_window(window_id) {
                return true;
            }
        }

        if let Some(window) = self.maximized_window
            && window_id == window.id
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
            container.load_focused_window();
            target_container_idx
        };

        let target_container = self
            .containers_mut()
            .get_mut(adjusted_target_container_index)
            .ok_or_eyre("there is no container")?;

        target_container.add_window(window);

        self.focus_container(adjusted_target_container_index);
        self.focused_container_mut()
            .ok_or_eyre("there is no container")?
            .load_focused_window();

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
            container.load_focused_window();
        }

        self.new_container_for_window(window);

        let mut container = Container::default();
        container.add_window(window);
        Ok(())
    }

    /// Hides every window of the workspace, or every one but `omit`: the floating windows and the
    /// windows of every container, the maximized window and the monocle container. A scrolling
    /// workspace is tiled again first without its resize adjustments, which are put back after,
    /// so that the windows are hidden where the layout has them and not where they were dragged.
    ///
    /// # Errors
    ///
    /// Never: the result is there so that callers can use `?`.
    pub fn hide(&mut self, omit: Option<u32>) -> eyre::Result<()> {
        for window in self.floating_windows_mut().iter_mut().rev() {
            let mut should_hide = omit.is_none();

            if !should_hide
                && let Some(omit) = omit
                && omit != window.id
            {
                should_hide = true
            }

            if should_hide {
                window.hide();
            }
        }

        let mut resize_dimensions = None;
        if matches!(self.layout, Layout::Default(DefaultLayout::Scrolling))
            && self.resize_dimensions.iter().any(|r| r.is_some())
        {
            resize_dimensions = Some(self.resize_dimensions.clone());
            self.resize_dimensions = Vec::new();
            if let Err(error) = self.update() {
                tracing::error!("{error}");
            }
        }

        for container in self.containers_mut() {
            container.hide(omit);
        }

        if let Some(resize_dimensions) = resize_dimensions {
            self.resize_dimensions = resize_dimensions;
        }

        if let Some(window) = self.maximized_window {
            window.hide();
        }

        if let Some(container) = &mut self.monocle_container {
            container.hide(omit);
        }

        Ok(())
    }

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
    pub fn restore(
        &mut self,
        mouse_follows_focus: bool,
        monitor_id: u32,
        monitor_wp: &Option<Wallpaper>,
    ) -> eyre::Result<()> {
        if let Some(container) = &mut self.monocle_container {
            container.restore();

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
            container.restore();
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

    /// Moves the focused container to the first index, the largest tile in every layout but the
    /// grid, and keeps the focus on it
    ///
    /// # Errors
    ///
    /// The workspace has no containers.
    pub fn promote_container(&mut self) -> eyre::Result<()> {
        let focused_idx = self.focused_container_idx();
        let container = self
            .containers_mut()
            .remove_respecting_locks(focused_idx)
            .ok_or_eyre("there is no container")?;

        let primary_idx = match &self.layout {
            Layout::Default(_) => 0,
        };

        let insertion_idx = self
            .containers_mut()
            .insert_respecting_locks(primary_idx, container);
        self.focus_container(insertion_idx);

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

    fn focus_last_container(&mut self) {
        self.focus_container(self.containers().len().saturating_sub(1));
    }

    fn focus_first_container(&mut self) {
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
            .load_focused_window();

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
            .load_focused_window();

        self.monocle_container = None;
        self.monocle_container_restore_idx = None;

        Ok(())
    }

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
    pub fn new_maximized_window(&mut self) -> eyre::Result<()> {
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
                monocle_container.load_focused_window();
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
            container.load_focused_window();
        }

        self.maximized_window = Option::from(window);
        self.maximized_window_restore_idx = Option::from(focused_idx);
        window.maximize()?;

        Ok(())
    }

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
    pub fn adopt_maximized_window(&mut self, window_id: u32) -> eyre::Result<()> {
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
                monocle_container.load_focused_window();
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
            container.load_focused_window();
        }

        self.maximized_window = Option::from(window);
        self.maximized_window_restore_idx = Option::from(container_idx);

        Ok(())
    }

    /// The maximized window is a tile again, in a container of its own at the index it was taken
    /// from, and the compositor gives it back the frame it had
    ///
    /// # Errors
    ///
    /// The workspace has no maximized window, or the compositor refuses to restore it.
    pub fn reintegrate_maximized_window(&mut self) -> eyre::Result<()> {
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
        container.add_window(window);

        // we shouldn't use insert_container_at_index here because it doesn't make sense for
        // monocle and maximized toggles which take over the whole screen before being reinserted
        // at the same index to respect locked container indexes
        self.containers_mut().insert(restore_idx, container);
        self.focus_container(restore_idx);

        self.maximized_window = None;
        self.maximized_window_restore_idx = None;

        Ok(())
    }

    /// Takes the focused window out of its container, and out of the layout, into the floating
    /// windows of the workspace; a container that is left with no window is removed, and one that
    /// is left with windows shows the one in front now
    ///
    /// # Errors
    ///
    /// The workspace has no containers, or the focused one has no window.
    pub fn new_floating_window(&mut self) -> eyre::Result<()> {
        let window = if let Some(maximized_window) = self.maximized_window {
            maximized_window.unmaximize()?;
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
                monocle_container.load_focused_window();
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
                container.load_focused_window();
            }

            window
        };

        self.floating_windows_mut().push_back(window);

        Ok(())
    }

    /// Puts the floating window that has the focus in the compositor back in the layout, in a
    /// container of its own at the index of the focused container
    ///
    /// # Errors
    ///
    /// The window the compositor has focused is not a floating window of the workspace.
    pub fn new_container_for_floating_window(&mut self) -> eyre::Result<()> {
        let focused_idx = self.focused_container_idx();
        let window = self
            .remove_focused_floating_window()
            .ok_or_eyre("there is no floating window")?;

        let mut container = Container::default();
        container.add_window(window);

        self.insert_container_at_idx(focused_idx, container);

        Ok(())
    }

    pub fn remove_focused_floating_window(&mut self) -> Option<Window> {
        let window_id = backend().foreground_window().and_then(WindowId::to_u32)?;
        self.remove_floating_window(window_id)
    }

    pub fn remove_floating_window(&mut self, window_id: u32) -> Option<Window> {
        let mut idx = None;
        for (i, window) in self.floating_windows().iter().enumerate() {
            if window_id == window.id {
                idx = Option::from(i);
            }
        }

        match idx {
            None => None,
            Some(idx) => {
                if self.floating_windows().get(idx).is_some() {
                    self.focus_previous_floating_window();
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
}

impl Workspace {
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
    pub fn update(&mut self) -> eyre::Result<()> {
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
                        self.scrolling_placed = Some(crate::scrolling::Placed::new(
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

    /// Returns the index of the container whose tile has the pointer in it, or `None` when the
    /// pointer is on no tile, or the backend cannot say where it is
    ///
    /// The tiles are those the layout last calculated ([`Workspace::latest_layout`]).
    pub fn container_idx_from_current_point(&self) -> Option<usize> {
        let mut idx = None;

        let point = backend().cursor_pos()?;

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
    pub fn container_for_window(&self, window_id: u32) -> Option<&Container> {
        self.containers()
            .get(self.container_idx_for_window(window_id)?)
    }

    /// The windows that are in view on this workspace: the maximized window (an entry that is
    /// `None` when there is none), the one at the top of the monocle container, the one at the top
    /// of each container, and every floating window
    pub fn visible_windows(&self) -> Vec<Option<&Window>> {
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

    /// What the compositor says about each window [`Workspace::visible_windows`] has, as
    /// `komorebic visible-windows` shows it: the maximized window, the one at the top of the
    /// monocle container, the one at the top of each container, and every floating window. A
    /// container that has no window is left out.
    pub fn visible_window_details(&self) -> Vec<WindowDetails> {
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

    /// Has the monitor show this workspace's wallpaper, or else the monitor's `monitor_wp`, and
    /// gives the borders the colors of the palette of the picture unless the wallpaper asks for no
    /// theme. Nothing is done for a workspace and a monitor that have no wallpaper.
    ///
    /// A failure to set the wallpaper is logged and does not make this fail, and the colors are
    /// still changed: the picture may be shown a moment later, when the Plasma shell is up. The
    /// colors are changed before this returns, so the borders drawn for the workspace that comes
    /// into view are right at once.
    pub fn apply_wallpaper(
        &self,
        monitor_id: u32,
        monitor_wp: &Option<Wallpaper>,
    ) -> eyre::Result<()> {
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

impl Workspace {
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
            _ => self.enforce_no_resize(),
        }
    }

    fn enforce_resize_constraints_for_bsp(&mut self) {
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

    fn enforce_resize_for_columns(&mut self) {
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

    fn enforce_resize_for_rows(&mut self) {
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

    fn enforce_resize_for_vertical_stack(&mut self) {
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

    fn enforce_resize_for_right_vertical_stack(&mut self) {
        let resize_dimensions = &mut self.resize_dimensions;
        match resize_dimensions.len() {
            // Single window can not be resized at all
            0 | 1 => self.enforce_no_resize(),
            _ => {
                // Zero is actually on the right
                if let Some(left) = resize_dimensions[1].as_mut() {
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

    fn enforce_resize_for_ultrawide(&mut self) {
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

    fn enforce_no_resize(&mut self) {
        for rect in self.resize_dimensions.iter_mut().flatten() {
            rect.left = 0;
            rect.right = 0;
            rect.top = 0;
            rect.bottom = 0;
        }
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
        ws.new_container_for_window(Window::from(123));

        // new length should be 5, with the focus on the new window at index 4
        assert_eq!(ws.containers().len(), 5);
        assert_eq!(ws.focused_container_idx(), 4);
        assert_eq!(
            ws.focused_container().unwrap().focused_window().unwrap().id,
            123
        );

        // when inserting a new container at index 0, index 3's container should not change
        ws.focus_container(0);
        ws.new_container_for_window(Window::from(234));
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
        container.add_window(window);
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
            workspace.new_container_for_window(Window::from(*id));
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

        with_backend(fake.clone(), || workspace.hide(Some(471)))?;
        assert!(!fake.is_hidden(471) && fake.is_hidden(472) && fake.is_hidden(473));

        with_backend(fake.clone(), || workspace.hide(None))?;
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
                container.hide(None);
            }
        });
        (fake, workspace)
    }

    #[test]
    fn showing_a_workspace_in_monocle_restores_the_monocle_window_and_no_other() -> eyre::Result<()>
    {
        let (fake, mut workspace) = workspace_in_monocle(&[481, 482, 483]);
        assert!(fake.is_hidden(481) && fake.is_hidden(482) && !fake.is_hidden(483));
        with_backend(fake.clone(), || workspace.hide(None))?;
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

        with_backend(fake.clone(), || workspace.hide(None))?;

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
