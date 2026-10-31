//! A recording stub platform for the characterization tests of `komorebi-core`.
//!
//! `StubWindow` and `StubPlatform` log every window operation and every hook call, in order, as a
//! line of text, so that a test says exactly which calls a shared function makes, with which
//! arguments. What the stub answers (the focus, the pointer, whether a call fails, what
//! `end_update` asks for) is set per test with the functions at the bottom of this file.
//!
//! The log and the settings are thread-local, and every test starts with [`fresh`], so tests do
//! not see each other whatever the harness does with threads.

#![allow(dead_code)]

use color_eyre::eyre;
use komorebi_config::matching::IdWithIdentifier;
use komorebi_config::matching::MatchingRule;
use komorebi_core::Container;
use komorebi_core::ContainerWindow;
use komorebi_core::Flow;
use komorebi_core::Monitor;
use komorebi_core::MonitorPlatform;
use komorebi_core::NoExtra;
use komorebi_core::Pass;
use komorebi_core::Placement;
use komorebi_core::Workspace;
use komorebi_core::WorkspacePlatform;
use komorebi_core::WorkspaceWindow;
use komorebi_layouts::OperationDirection;
use komorebi_layouts::Rect;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;

/// A window that is `Clone` but not `Copy`, as the macOS window is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StubWindow {
    pub id: u32,
}

/// The platform state the stub keeps in a workspace: the spot it hides windows at, which is how
/// macOS keeps the corner of its monitor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StubState {
    pub spot: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StubPlatform;

/// What the stub answers and what it makes fail; reset by [`fresh`].
#[derive(Default)]
struct Settings {
    foreground: Option<u32>,
    cursor: Option<(i32, i32)>,
    /// operations that fail, by the text the log would have for them (see `fails`)
    failing: HashSet<String>,
    /// what `begin_update` does: 0 runs, 1 skips
    skip_update: bool,
    /// what the successive calls of `end_update` answer; `Done` once it is empty
    passes: Vec<Pass>,
    adopt_widths: bool,
    scrolling_insertion_idx: Option<usize>,
    /// what `scrolling_layouts` hides
    scrolling_hidden: Vec<bool>,
    known_names: HashMap<usize, String>,
    /// how much `adjust_layout` shrinks a tile
    adjust: i32,
    /// the id the placement rule matchers answer true for
    match_id: Option<String>,
}

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static SETTINGS: RefCell<Settings> = RefCell::new(Settings::default());
}

/// Starts a test: empties the log and sets the stub back to its defaults (the focus is on no
/// window, nothing fails, `end_update` is done after one pass).
pub fn fresh() {
    LOG.with(|log| log.borrow_mut().clear());
    SETTINGS.with(|s| *s.borrow_mut() = Settings::default());
}

/// Takes the log, leaving it empty.
pub fn log() -> Vec<String> {
    LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

fn push(line: String) {
    LOG.with(|log| log.borrow_mut().push(line));
}

fn settings<T>(f: impl FnOnce(&mut Settings) -> T) -> T {
    SETTINGS.with(|s| f(&mut s.borrow_mut()))
}

/// Makes the operation fail whenever it is called: the line the log has for it, without the
/// arguments that follow the name, e.g. `fail("hide 3")` (a window operation names the window) or
/// `fail("place_container")` (a hook).
pub fn fail(operation: &str) {
    settings(|s| s.failing.insert(operation.to_string()));
}

pub fn set_foreground(id: Option<u32>) {
    settings(|s| s.foreground = id);
}

pub fn set_cursor(point: Option<(i32, i32)>) {
    settings(|s| s.cursor = point);
}

pub fn skip_updates() {
    settings(|s| s.skip_update = true);
}

/// What the successive calls of `end_update` answer.
pub fn script_passes(passes: &[Pass]) {
    settings(|s| s.passes = passes.iter().rev().copied().collect());
}

pub fn adopt_widths(yes: bool) {
    settings(|s| s.adopt_widths = yes);
}

pub fn set_scrolling_insertion_idx(idx: Option<usize>) {
    settings(|s| s.scrolling_insertion_idx = idx);
}

pub fn set_scrolling_hidden(hidden: &[bool]) {
    settings(|s| s.scrolling_hidden = hidden.to_vec());
}

pub fn set_known_workspace_name(idx: usize, name: &str) {
    settings(|s| s.known_names.insert(idx, name.to_string()));
}

pub fn set_match_id(id: &str) {
    settings(|s| s.match_id = Some(id.to_string()));
}

pub fn set_adjust(by: i32) {
    settings(|s| s.adjust = by);
}

fn fails(operation: &str) -> bool {
    settings(|s| s.failing.contains(operation))
}

fn op(line: String, key: String) -> eyre::Result<()> {
    push(line);
    if fails(&key) {
        Err(eyre::eyre!("{key} failed"))
    } else {
        Ok(())
    }
}

impl ContainerWindow for StubWindow {
    type Id = u32;
    type HideSpot = u8;

    fn id(&self) -> u32 {
        self.id
    }

    fn exe_name(&self) -> Option<String> {
        Some(format!("exe{}", self.id))
    }

    fn hide_window(&self, spot: u8) -> eyre::Result<()> {
        op(
            format!("hide {} @{spot}", self.id),
            format!("hide {}", self.id),
        )
    }

    fn restore_window(&self) -> eyre::Result<()> {
        op(
            format!("restore {}", self.id),
            format!("restore {}", self.id),
        )
    }

    fn hide_stacked(&self, spot: u8) -> eyre::Result<()> {
        op(
            format!("hide_stacked {} @{spot}", self.id),
            format!("hide_stacked {}", self.id),
        )
    }

    fn restore_stacked(&self) -> eyre::Result<()> {
        op(
            format!("restore_stacked {}", self.id),
            format!("restore_stacked {}", self.id),
        )
    }
}

impl WorkspaceWindow for StubWindow {
    fn title(&self) -> Option<String> {
        Some(format!("title{}", self.id))
    }

    fn exe(&self) -> Option<String> {
        self.exe_name()
    }

    fn class(&self) -> Option<String> {
        Some("class".to_string())
    }

    fn path(&self) -> Option<String> {
        Some(format!("/bin/exe{}", self.id))
    }
}

pub fn rect_text(rect: &Rect) -> String {
    format!(
        "[{} {} {} {}]",
        rect.left, rect.top, rect.right, rect.bottom
    )
}

impl WorkspacePlatform for StubPlatform {
    type Window = StubWindow;
    type State = StubState;

    fn hide_spot(workspace: &Workspace<Self>) -> u8 {
        workspace.state.spot
    }

    /// a display spot is the spot plus 100, so that a test sees which of the two was passed
    fn hide_spot_on_display(workspace: &Workspace<Self>) -> u8 {
        workspace.state.spot + 100
    }

    fn foreground_window_id() -> Option<u32> {
        settings(|s| s.foreground)
    }

    fn cursor_position() -> Option<(i32, i32)> {
        settings(|s| s.cursor)
    }

    fn should_act_window(
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        rules: &[MatchingRule],
    ) -> Option<MatchingRule> {
        let id = settings(|s| s.match_id.clone())?;
        rules
            .iter()
            .find(|rule| matches!(rule, MatchingRule::Simple(r) if r.id == id))
            .cloned()
    }

    fn should_act_window_individual(
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        rule: &IdWithIdentifier,
    ) -> bool {
        settings(|s| s.match_id.clone()).is_some_and(|id| id == rule.id)
    }

    fn scrolling_idx_for_direction(_: &Workspace<Self>, _: OperationDirection) -> Option<usize> {
        None
    }

    fn scrolling_insertion_idx(_: &Workspace<Self>) -> Option<usize> {
        settings(|s| s.scrolling_insertion_idx)
    }

    fn note_scrolling_departure(_: &Workspace<Self>, idx: usize) {
        push(format!("note_scrolling_departure {idx}"));
    }

    fn after_floating_removed(workspace: &mut Workspace<Self>) {
        push(format!(
            "after_floating_removed len={} focus={}",
            workspace.floating_windows().len(),
            workspace.focused_floating_window_idx()
        ));
    }

    fn unmaximize_window(window: &StubWindow) -> eyre::Result<()> {
        op(
            format!("unmaximize {}", window.id),
            format!("unmaximize {}", window.id),
        )
    }

    fn before_hide_containers(_: &mut Workspace<Self>) -> Option<Vec<Option<Rect>>> {
        push("before_hide_containers".to_string());
        None
    }

    fn begin_update(_: &mut Workspace<Self>) -> eyre::Result<Flow> {
        push("begin_update".to_string());
        if fails("begin_update") {
            return Err(eyre::eyre!("begin_update failed"));
        }
        if settings(|s| s.skip_update) {
            return Ok(Flow::Skip);
        }
        Ok(Flow::Run(Some(Box::new(DropLogger))))
    }

    fn containers_pruned(_: &mut Workspace<Self>, focused_before: usize) {
        push(format!("containers_pruned focused_before={focused_before}"));
    }

    fn place_monocle(window: &mut StubWindow, rect: &Rect) -> eyre::Result<()> {
        op(
            format!("place_monocle {} {}", window.id, rect_text(rect)),
            "place_monocle".to_string(),
        )
    }

    fn maximize_in_layout(window: &mut StubWindow) -> eyre::Result<()> {
        op(
            format!("maximize_in_layout {}", window.id),
            "maximize_in_layout".to_string(),
        )
    }

    fn adopt_widths_before_layout(_: &Workspace<Self>) -> bool {
        push("adopt_widths_before_layout".to_string());
        settings(|s| s.adopt_widths)
    }

    fn adopt_actual_widths(
        _: &mut Workspace<Self>,
        around_window: i32,
        viewport_width: i32,
    ) -> bool {
        push(format!(
            "adopt_actual_widths around={around_window} viewport={viewport_width}"
        ));
        false
    }

    fn scrolling_layouts(
        workspace: &mut Workspace<Self>,
        adjusted_work_area: &Rect,
        container_padding: i32,
    ) -> (Vec<Rect>, Vec<bool>) {
        push(format!(
            "scrolling_layouts {} padding={container_padding}",
            rect_text(adjusted_work_area)
        ));
        let count = workspace.containers().len() as i32;
        let width = adjusted_work_area.right / count.max(1);
        let layouts = (0..count)
            .map(|i| Rect {
                left: adjusted_work_area.left + i * width,
                top: adjusted_work_area.top,
                right: width,
                bottom: adjusted_work_area.bottom,
            })
            .collect();
        (layouts, settings(|s| s.scrolling_hidden.clone()))
    }

    fn adjust_layout(
        container: &mut Container<StubWindow>,
        layout: &mut Rect,
        placement: &Placement,
    ) {
        push(format!(
            "adjust_layout {} {} scrolling={} maximized={} resize_empty={}",
            window_ids(container),
            rect_text(layout),
            placement.is_scrolling,
            placement.managed_maximized_window,
            placement.resize_dimensions_is_empty,
        ));
        let by = settings(|s| s.adjust);
        layout.add_padding(by);
    }

    fn place_container(
        container: &mut Container<StubWindow>,
        layout: &Rect,
        hidden: bool,
        placement: &Placement,
    ) -> eyre::Result<()> {
        op(
            format!(
                "place_container {} {} hidden={hidden} padding={} border={}+{}",
                window_ids(container),
                rect_text(layout),
                placement.container_padding,
                placement.border_offset,
                placement.border_width,
            ),
            "place_container".to_string(),
        )
    }

    fn end_update(
        _: &mut Workspace<Self>,
        pass: usize,
        around_window: i32,
        viewport_width: i32,
    ) -> eyre::Result<Pass> {
        push(format!(
            "end_update pass={pass} around={around_window} viewport={viewport_width}"
        ));
        if fails("end_update") {
            return Err(eyre::eyre!("end_update failed"));
        }
        Ok(settings(|s| s.passes.pop()).unwrap_or(Pass::Done))
    }
}

/// The guard `begin_update` hands out, which logs when the update is over.
struct DropLogger;

impl Drop for DropLogger {
    fn drop(&mut self) {
        push("guard dropped".to_string());
    }
}

fn window_ids(container: &Container<StubWindow>) -> String {
    let ids: Vec<String> = container
        .windows()
        .iter()
        .map(|w| w.id.to_string())
        .collect();
    format!("({})", ids.join(","))
}

impl MonitorPlatform for StubPlatform {
    type Id = u32;
    type SerialNumber = String;
    type Extra = NoExtra;
    #[cfg(feature = "schemars")]
    type IdSchema = u32;

    fn serialize_id<S: Serializer>(id: &u32, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(*id)
    }

    fn deserialize_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
        u32::deserialize(deserializer)
    }

    fn known_workspace_name(_: &Monitor<Self>, idx: usize) -> Option<String> {
        settings(|s| s.known_names.get(&idx).cloned())
    }
}

/// A container of the given window ids, the last one in front.
pub fn container_of(ids: &[u32]) -> Container<StubWindow> {
    let mut container = Container::default();
    for id in ids {
        container.add_window(StubWindow { id: *id }, 0).unwrap();
    }
    container
}

/// A workspace with one container per window id, in order, the last one focused; the log is
/// emptied afterwards.
pub fn workspace_of(ids: &[u32]) -> Workspace<StubPlatform> {
    let mut workspace = Workspace::<StubPlatform>::default();
    for id in ids {
        workspace
            .new_container_for_window(StubWindow { id: *id })
            .unwrap();
    }
    log();
    workspace
}

/// The window ids of every container, in order.
pub fn layout_of(workspace: &Workspace<StubPlatform>) -> Vec<Vec<u32>> {
    workspace
        .containers()
        .iter()
        .map(|c| c.windows().iter().map(|w| w.id).collect())
        .collect()
}

pub fn work_area(right: i32, bottom: i32) -> Rect {
    Rect {
        left: 0,
        top: 0,
        right,
        bottom,
    }
}
pub mod fixtures;
