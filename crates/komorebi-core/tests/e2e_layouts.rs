//! What the Windows `layouts`, `flipping`, `layout_flip`, `padding`, `closing`, `tiling` and
//! `stacks` scenarios (shared with KDE, and the only place macOS-or-Windows tiling used to be
//! checked) assert about where `Workspace::update_shared` puts the windows, run on the stub
//! platform: the tiles the shared code hands to the platform (`latest_layout`, which the scenarios
//! compare the OS frames with) for every layout, count, padding and flip.
//!
//! These protect all three daemons (Windows, KDE, macOS: each calls `update_shared`). Platform
//! choice: the stub platform adds no border and adjusts nothing, so a daemon's own `adjust_layout`
//! (the Windows border of 7 px, macOS or KDE frame corrections) is NOT covered and stays e2e
//! (`windows_padding`).
//!
//! The Windows fixture `kfw-tiled.json` of the e2e library is the same state shape these
//! workspaces serialize to (see `state_roundtrip.rs`).

mod common;

use common::*;
use komorebi_core::Workspace;
use komorebi_layouts::Axis;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::Layout;
use komorebi_layouts::Rect;

const LAYOUTS: [DefaultLayout; 8] = [
    DefaultLayout::BSP,
    DefaultLayout::Columns,
    DefaultLayout::Rows,
    DefaultLayout::VerticalStack,
    DefaultLayout::HorizontalStack,
    DefaultLayout::UltrawideVerticalStack,
    DefaultLayout::Grid,
    DefaultLayout::RightMainVerticalStack,
];

const AREA: Rect = Rect {
    left: 0,
    top: 0,
    right: 1920,
    bottom: 1080,
};

fn workspace(
    layout: DefaultLayout,
    ids: &[u32],
    container_padding: Option<i32>,
    workspace_padding: Option<i32>,
    flip: Option<Axis>,
) -> Workspace<StubPlatform> {
    let mut workspace = workspace_of(ids);
    workspace.layout = Layout::Default(layout);
    workspace.layout_flip = flip;
    workspace.globals.work_area = AREA;
    workspace.globals.container_padding = container_padding;
    workspace.globals.workspace_padding = workspace_padding;
    workspace
}

fn tiles(
    layout: DefaultLayout,
    count: u32,
    container_padding: Option<i32>,
    workspace_padding: Option<i32>,
    flip: Option<Axis>,
) -> Vec<Rect> {
    fresh();
    let ids: Vec<u32> = (1..=count).collect();
    let mut workspace = workspace(layout, &ids, container_padding, workspace_padding, flip);
    workspace.update_shared().unwrap();
    workspace.latest_layout.clone()
}

fn inside(r: &Rect, a: &Rect) -> bool {
    r.left >= a.left
        && r.top >= a.top
        && r.left + r.right <= a.left + a.right
        && r.top + r.bottom <= a.top + a.bottom
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.left < b.left + b.right
        && b.left < a.left + a.right
        && a.top < b.top + b.bottom
        && b.top < a.top + a.bottom
}

fn area(r: &Rect) -> i64 {
    i64::from(r.right) * i64::from(r.bottom)
}

// mirrors: layouts / <layout>, <n> windows: every window is where the layout says
// mirrors: layouts / ... inside the work area
// mirrors: layouts / ... and not on each other
// mirrors: tiling / every window is where the layout says; <a> and <b> do not overlap
#[test]
fn every_layout_tiles_every_container_inside_the_work_area_and_off_the_others() {
    for layout in LAYOUTS {
        for count in 1..=8 {
            for (cp, wp) in [(None, None), (Some(10), Some(20)), (Some(0), Some(0))] {
                let t = tiles(layout, count, cp, wp, None);
                assert_eq!(t.len(), count as usize, "{layout} {count}");
                for (i, r) in t.iter().enumerate() {
                    assert!(
                        inside(r, &AREA),
                        "{layout} {count} {cp:?}/{wp:?}: {i} {r:?}"
                    );
                    assert!(
                        r.right > 0 && r.bottom > 0,
                        "{layout} {count}: {i} is empty"
                    );
                    for (j, o) in t.iter().enumerate().skip(i + 1) {
                        assert!(!overlap(r, o), "{layout} {count}: {i} {r:?} over {j} {o:?}");
                    }
                }
            }
        }
    }
}

// mirrors: padding / a workspace padding 40 px more moves the windows 40 px in from the edge
// mirrors: padding / a container padding 30 px more leaves the windows less room
// mirrors: padding / with the padding as it was, the windows are as far from the edge as they were
#[test]
fn the_workspace_padding_moves_the_tiles_in_and_the_container_padding_shrinks_them() {
    for layout in LAYOUTS {
        let base = tiles(layout, 3, None, None, None);
        let more = tiles(layout, 3, None, Some(40), None);
        let edge = |t: &[Rect]| t.iter().map(|r| r.left).min().unwrap();
        assert_eq!(
            edge(&more),
            edge(&base) + 40,
            "{layout}: workspace padding 40"
        );
        assert!(inside(more.first().unwrap(), &AREA));

        let padded = tiles(layout, 3, Some(30), None, None);
        for (a, b) in base.iter().zip(&padded) {
            assert!(
                area(b) < area(a),
                "{layout}: container padding 30 did not shrink {a:?}"
            );
        }

        assert_eq!(
            base,
            tiles(layout, 3, None, None, None),
            "{layout}: back as it was"
        );
    }
}

// mirrors: flipping / after 'flip-layout horizontal' every window is where the mirror of its place is
// mirrors: flipping / flipped again, every window is where it was
// mirrors: layout_flip / <label>: the daemon starts with the layout mirrored
// mirrors: layout_flip / 'flip-layout vertical' adds to it: flipped along both axes
#[test]
fn a_flip_mirrors_the_tiles_of_the_workspace_as_a_set() {
    let mirror = |r: &Rect, axis: Axis| {
        let mut m = *r;
        if matches!(axis, Axis::Horizontal | Axis::HorizontalAndVertical) {
            m.left = AREA.left + AREA.right - (r.left + r.right);
        }
        if matches!(axis, Axis::Vertical | Axis::HorizontalAndVertical) {
            m.top = AREA.top + AREA.bottom - (r.top + r.bottom);
        }
        m
    };
    let key = |r: &Rect| (r.left, r.top, r.right, r.bottom);
    for layout in LAYOUTS {
        for count in 1..=6 {
            let plain = tiles(layout, count, Some(10), Some(20), None);
            for axis in [
                Axis::Horizontal,
                Axis::Vertical,
                Axis::HorizontalAndVertical,
            ] {
                let mut flipped = tiles(layout, count, Some(10), Some(20), Some(axis));
                let mut want: Vec<Rect> = plain.iter().map(|r| mirror(r, axis)).collect();
                // the workspace padding is symmetric, so the mirror of the padded tiles is the
                // padded mirror
                flipped.sort_by_key(key);
                want.sort_by_key(key);
                assert_eq!(flipped, want, "{layout} {count} {axis}");
            }
        }
    }
}

// mirrors: closing / the windows that are left take more room between them
// mirrors: minimizing / with <target> minimized the others take more room between them
// mirrors: floating / ... the others take more room between them
#[test]
fn removing_a_container_gives_the_others_more_room() {
    for layout in LAYOUTS.into_iter().filter(|l| *l != DefaultLayout::Grid) {
        fresh();
        let mut w = workspace(layout, &[1, 2, 3, 4], Some(10), Some(20), None);
        w.update_shared().unwrap();
        let before: i64 = w.latest_layout.iter().map(area).max().unwrap();
        // the focused (last) container leaves, as a closed, minimized or floated window does
        w.remove_focused_container();
        w.update_shared().unwrap();
        assert_eq!(w.latest_layout.len(), 3, "{layout}");
        let after: i64 = w.latest_layout.iter().map(area).max().unwrap();
        assert!(after >= before, "{layout}: {before} -> {after}");
        for (i, r) in w.latest_layout.iter().enumerate() {
            assert!(inside(r, &AREA), "{layout}: {i} {r:?}");
        }
    }
}

// mirrors: stacks / two containers, one of them a stack of two  (three windows, two tiles)
// mirrors: stacks / and they tile, not on each other
#[test]
fn a_stack_is_one_tile() {
    for layout in LAYOUTS {
        fresh();
        let mut w = workspace(layout, &[1, 2, 3], Some(10), Some(20), None);
        // stack window 3 onto container 1 as `stack left` does
        w.focus_container(2);
        let _ = w.move_window_to_container(0);
        w.update_shared().unwrap();
        assert_eq!(w.containers().len(), 2, "{layout}");
        assert_eq!(w.latest_layout.len(), 2, "{layout}");
        assert!(
            !overlap(&w.latest_layout[0], &w.latest_layout[1]),
            "{layout}"
        );
    }
}

// mirrors: tiling_off / after 'toggle-tiling' ... no window moved
// mirrors: tiling_off / ... a layout change does not move them either
#[test]
fn a_workspace_with_tiling_off_places_nothing_whatever_the_layout() {
    for layout in LAYOUTS {
        fresh();
        let mut w = workspace(layout, &[1, 2, 3], Some(10), Some(20), None);
        w.tile = false;
        w.update_shared().unwrap();
        assert!(
            !log().iter().any(|l| l.starts_with("place_container")),
            "{layout}: a window was placed"
        );
    }
}
