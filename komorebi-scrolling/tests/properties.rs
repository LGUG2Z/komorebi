//! Random sequences of every operation, checked after each step against the properties that must
//! hold whatever has been done: the strip is laid out sanely, the focused column is in view, and
//! the focused column only moves when something to do with it happens.
//!
//! The sequences are generated from a seed, so a failure names the seed and prints the steps,
//! and can be replayed exactly.

use komorebi_scrolling::CenterFocusedColumn;
use komorebi_scrolling::ColumnWidth;
use komorebi_scrolling::Options;
use komorebi_scrolling::ScrollingSpace;
use komorebi_scrolling::TileHeight;
use komorebi_scrolling::Viewport;
use komorebi_scrolling::Visibility;
use std::collections::HashMap;

const SEEDS: u64 = 600;
const STEPS: usize = 90;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // a linear congruential generator is plenty for choosing between a few options
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> T {
        options[self.below(options.len())]
    }
}

fn random_width(rng: &mut Rng) -> ColumnWidth {
    if rng.below(3) == 0 {
        ColumnWidth::Fixed(rng.pick(&[150, 400, 700, 1000, 1700]))
    } else {
        ColumnWidth::Proportion(rng.pick(&[0.2, 0.25, 1.0 / 3.0, 0.5, 0.75, 1.0, 1.5]))
    }
}

/// What the strip looked like before a step.
struct Before {
    active: Option<u32>,
    active_x: Option<f64>,
    widths: HashMap<u32, i32>,
}

impl Before {
    fn of(space: &ScrollingSpace<u32>) -> Self {
        Self {
            active: space.active_id().copied(),
            active_x: space.active_x(),
            widths: space
                .layout()
                .into_iter()
                .map(|placement| (placement.id, placement.rect.width))
                .collect(),
        }
    }
}

/// What a step promises about what it leaves alone.
struct Promise {
    /// The active column is the same one and is in the same place.
    active_stays: bool,
    /// The columns whose width the step is allowed to change. `None` means any may change.
    widths_that_may_change: Option<Vec<u32>>,
}

impl Promise {
    fn nothing_moves() -> Self {
        Self {
            active_stays: true,
            widths_that_may_change: Some(vec![]),
        }
    }
}

fn step(
    rng: &mut Rng,
    space: &mut ScrollingSpace<u32>,
    next_id: &mut u32,
    log: &mut Vec<String>,
) -> Promise {
    if space.is_empty() {
        let id = *next_id;
        *next_id += 1;
        log.push(format!("add {id} to the empty strip"));
        space.add_column(id, None, None, true);

        return Promise {
            active_stays: false,
            widths_that_may_change: Some(vec![]),
        };
    }

    let ids = space.ids();
    let count = ids.len();
    let active = *space.active_id().unwrap();
    let target = ids[rng.below(count)];
    // a column is one thing: what happens to a tile happens to the column that it is in
    let in_active_column = space.position(&target) == space.position(&active);

    match rng.below(29) {
        0 | 1 => {
            let id = *next_id;
            *next_id += 1;
            let activate = rng.below(2) == 0;
            let at = (rng.below(3) == 0).then(|| rng.below(count + 1));
            let width = (rng.below(3) == 0).then(|| random_width(rng));
            log.push(format!(
                "add {id} at {at:?} width {width:?} activate {activate}"
            ));
            space.add_column(id, at, width, activate);

            Promise {
                active_stays: !activate,
                widths_that_may_change: Some(vec![]),
            }
        }
        2 => {
            log.push(format!("remove {target}"));
            space.remove_column(&target);

            Promise {
                active_stays: target != active,
                widths_that_may_change: Some(vec![]),
            }
        }
        3 | 4 => {
            log.push(format!("focus {target}"));
            space.focus(&target);

            Promise {
                active_stays: target == active,
                widths_that_may_change: Some(vec![]),
            }
        }
        5 => {
            let width = random_width(rng);
            log.push(format!("set width of {target} to {width:?}"));
            space.set_width(&target, width);

            Promise {
                active_stays: !in_active_column,
                widths_that_may_change: Some(vec![target]),
            }
        }
        6 => {
            let forward = rng.below(2) == 0;
            log.push(format!("cycle the preset of {target}, forward {forward}"));
            space.cycle_preset_width(&target, forward);

            Promise {
                active_stays: !in_active_column,
                widths_that_may_change: Some(vec![target]),
            }
        }
        7 => {
            log.push(format!("toggle full width of {target}"));
            space.toggle_full_width(&target);

            Promise {
                active_stays: !in_active_column,
                widths_that_may_change: Some(vec![target]),
            }
        }
        8 => {
            let fullscreen = rng.below(2) == 0;
            log.push(format!("set fullscreen of {target} to {fullscreen}"));
            space.set_fullscreen(&target, fullscreen);

            Promise {
                active_stays: !in_active_column,
                widths_that_may_change: Some(vec![target]),
            }
        }
        9 => {
            let min = (rng.below(2) == 0).then(|| rng.pick(&[100, 500, 900]));
            // a maximum below the minimum is not something a window can report
            let max = if min.is_some() {
                None
            } else {
                (rng.below(2) == 0).then(|| rng.pick(&[300, 800, 1300]))
            };
            log.push(format!("limit {target} to {min:?} and {max:?}"));
            space.set_width_limits(&target, min, max);

            Promise {
                active_stays: !in_active_column,
                widths_that_may_change: Some(vec![target]),
            }
        }
        10 => {
            let to = rng.below(count);
            log.push(format!("move {target} to {to}"));
            space.move_column(&target, to);

            Promise {
                active_stays: !in_active_column,
                widths_that_may_change: Some(vec![]),
            }
        }
        11 => {
            log.push("centre the column".into());
            space.center_column();

            Promise {
                active_stays: false,
                widths_that_may_change: Some(vec![]),
            }
        }
        12 => {
            log.push("centre the visible columns".into());
            space.center_visible_columns();

            Promise {
                active_stays: false,
                widths_that_may_change: Some(vec![]),
            }
        }
        13 => {
            log.push("expand to the available width".into());
            space.expand_to_available_width();

            Promise {
                active_stays: false,
                widths_that_may_change: Some(vec![active]),
            }
        }
        14 => {
            let viewport = rng.pick(&[
                Viewport {
                    width: 1200,
                    height: 800,
                },
                Viewport {
                    width: 900,
                    height: 700,
                },
                Viewport {
                    width: 1600,
                    height: 900,
                },
            ]);
            log.push(format!("set the viewport to {viewport:?}"));
            space.set_viewport(viewport);

            Promise {
                active_stays: false,
                widths_that_may_change: None,
            }
        }
        15 => {
            log.push("reset the widths".into());
            space.reset_widths();

            Promise {
                active_stays: false,
                widths_that_may_change: None,
            }
        }
        16 => {
            log.push("consume to the right".into());
            space.consume_right();

            // the tile that was taken has the width of the column that took it
            Promise {
                active_stays: true,
                widths_that_may_change: None,
            }
        }
        17 => {
            log.push("consume to the left".into());
            space.consume_left();

            Promise {
                active_stays: true,
                widths_that_may_change: None,
            }
        }
        18 => {
            log.push("expel".into());
            space.expel();

            Promise {
                active_stays: false,
                widths_that_may_change: None,
            }
        }
        19 => {
            log.push("focus up".into());
            space.focus_up();

            Promise {
                active_stays: false,
                widths_that_may_change: Some(vec![]),
            }
        }
        20 => {
            log.push("focus down".into());
            space.focus_down();

            Promise {
                active_stays: false,
                widths_that_may_change: Some(vec![]),
            }
        }
        21 => {
            log.push("move the tile up".into());
            space.move_tile_up();

            Promise {
                active_stays: true,
                widths_that_may_change: Some(vec![]),
            }
        }
        22 => {
            log.push("move the tile down".into());
            space.move_tile_down();

            Promise {
                active_stays: true,
                widths_that_may_change: Some(vec![]),
            }
        }
        23 => {
            let dx = rng.pick(&[-700.0, -250.0, -40.0, 40.0, 250.0, 700.0]);
            let velocity = rng.pick(&[0.0, 300.0, -300.0, 3000.0, -3000.0]);
            log.push(format!(
                "pan by {dx} and then {}, let go at {velocity}",
                dx / 2.0
            ));
            space.begin_pan();
            space.pan_by(dx);
            space.pan_by(dx / 2.0);
            space.pan_end(velocity);

            Promise {
                active_stays: false,
                widths_that_may_change: Some(vec![]),
            }
        }
        24 => {
            let height = rng.pick(&[
                TileHeight::Auto,
                TileHeight::Fixed(90),
                TileHeight::Fixed(500),
                TileHeight::Fixed(3000),
                TileHeight::Proportion(0.2),
                TileHeight::Proportion(0.9),
            ]);
            log.push(format!("give {target} the height {height:?}"));
            space.set_tile_height(&target, height);

            Promise {
                active_stays: true,
                widths_that_may_change: Some(vec![]),
            }
        }
        25 => {
            let delta = rng.pick(&[-400.0, -60.0, 60.0, 400.0]);
            log.push(format!("resize the height of {target} by {delta}"));
            space.resize_tile_height(&target, delta);

            Promise {
                active_stays: true,
                widths_that_may_change: Some(vec![]),
            }
        }
        26 => {
            let forward = rng.below(2) == 0;
            log.push(format!(
                "cycle the preset height of {target}, forward {forward}"
            ));
            space.cycle_preset_height(&target, forward);

            Promise {
                active_stays: true,
                widths_that_may_change: Some(vec![]),
            }
        }
        27 => {
            log.push(format!("reset the heights of the column of {target}"));
            space.reset_tile_heights(&target);

            Promise {
                active_stays: true,
                widths_that_may_change: Some(vec![]),
            }
        }
        _ => {
            // the host reports the strip as it is: that must change nothing at all
            let ids = space.ids();
            let focused = space.active_id().copied();
            log.push("reconcile with exactly what is there".into());
            space.reconcile(&ids, focused.as_ref());

            Promise::nothing_moves()
        }
    }
}

fn check(
    space: &ScrollingSpace<u32>,
    before: &Before,
    promise: &Promise,
    gaps: i32,
) -> Result<(), String> {
    let layout = space.layout();
    let viewport = space.viewport();

    // the strip is ordered, and its keys are what they say they are
    let ids = space.ids();
    if layout
        .iter()
        .map(|placement| placement.id)
        .collect::<Vec<_>>()
        != ids
    {
        return Err("the layout is not in the order of the keys".into());
    }
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() != ids.len() {
        return Err("a key is in the strip twice".into());
    }

    // columns do not overlap, and the space between them is the gap, give or take rounding. Tiles
    // of a column are one above the other with a gap between, or none if it is fullscreen
    for pair in layout.windows(2) {
        if pair[1].rect.x == pair[0].rect.x {
            let between = pair[1].rect.y - pair[0].rect.bottom();
            if pair[1].rect.width != pair[0].rect.width
                || pair[1].visibility != pair[0].visibility
                || !(-1..=gaps + 1).contains(&between)
            {
                return Err(format!(
                    "tiles {} and {} are not stacked: {:?} and {:?}",
                    pair[0].id, pair[1].id, pair[0].rect, pair[1].rect
                ));
            }

            continue;
        }

        let between = pair[1].rect.x - pair[0].rect.right();
        if (between - gaps).abs() > 1 {
            return Err(format!(
                "there are {between} pixels between columns {} and {}, and the gap is {gaps}",
                pair[0].id, pair[1].id
            ));
        }
    }

    // the tiles of a column fill it, whatever they were given: the last ends where the column does
    for (index, placement) in layout.iter().enumerate() {
        let last_in_column = layout
            .get(index + 1)
            .is_none_or(|next| next.rect.x != placement.rect.x);

        if last_in_column
            && placement.rect.bottom()
                != layout
                    .iter()
                    .filter(|p| p.rect.x == placement.rect.x)
                    .map(|p| p.rect.bottom())
                    .max()
                    .unwrap_or(0)
        {
            return Err("the last tile of a column is not the lowest".into());
        }

        let column: Vec<_> = layout
            .iter()
            .filter(|p| p.rect.x == placement.rect.x)
            .collect();
        if last_in_column && column.len() > 1 {
            let end = placement.rect.bottom();
            let expected = if column[0].rect.y == 0 {
                viewport.height
            } else {
                viewport.height - gaps
            };
            if (end - expected).abs() > 1 {
                return Err(format!(
                    "the tiles of the column at {} end at {end} and the column ends at {expected}",
                    placement.rect.x
                ));
            }
        }
    }

    for placement in &layout {
        if placement.rect.width < 1 || placement.rect.height < 1 {
            return Err(format!("column {} has an empty rectangle", placement.id));
        }
    }

    if layout.is_empty() {
        return Ok(());
    }

    // exactly one column is active, and it is in view, or starts at the left edge if it is too wide
    let active: Vec<_> = layout.iter().filter(|placement| placement.active).collect();
    if active.len() != 1 {
        return Err(format!("{} columns are active", active.len()));
    }

    let active = active[0];
    if Some(&active.id) != space.active_id() {
        return Err("the active column is not the one that is reported as active".into());
    }

    if active.rect.width >= viewport.width {
        if active.rect.x != 0 {
            return Err(format!(
                "the active column is as wide as the viewport or wider, and starts at {}",
                active.rect.x
            ));
        }
    } else if active.visibility != Visibility::Visible {
        return Err(format!(
            "the active column {} is {:?} at {:?}",
            active.id, active.visibility, active.rect
        ));
    }

    // what a step promised to leave alone
    if promise.active_stays {
        if before.active != space.active_id().copied() {
            return Err(format!(
                "the active column changed from {:?} to {:?}",
                before.active,
                space.active_id()
            ));
        }
        if before.active_x != space.active_x() {
            return Err(format!(
                "the active column moved from {:?} to {:?}",
                before.active_x,
                space.active_x()
            ));
        }
    }

    if let Some(allowed) = &promise.widths_that_may_change {
        // a column has one width, so a step that may change the width of a tile may change the
        // width of the tiles that it is in a column with
        let columns: Vec<i32> = layout
            .iter()
            .filter(|placement| allowed.contains(&placement.id))
            .map(|placement| placement.rect.x)
            .collect();

        for placement in &layout {
            if allowed.contains(&placement.id) || columns.contains(&placement.rect.x) {
                continue;
            }

            if let Some(was) = before.widths.get(&placement.id)
                && (placement.rect.width - was).abs() > 1
            {
                return Err(format!(
                    "the width of column {} changed from {was} to {}",
                    placement.id, placement.rect.width
                ));
            }
        }
    }

    // saying what is there changes nothing: not what is where, and not anything that the strip
    // remembers either
    let mut again = space.clone();
    again.reconcile(&ids, space.active_id());
    if again.layout() != layout {
        return Err("reconciling with exactly what is there changed the layout".into());
    }
    if again != *space {
        return Err(
            "reconciling with exactly what is there changed what the strip remembers".into(),
        );
    }

    // the active tile is where reading order says
    let index = layout.iter().position(|placement| placement.active);
    if index != space.active_tile_index() {
        return Err(format!(
            "the active tile is at {index:?} in the layout and at {:?} by the strip",
            space.active_tile_index()
        ));
    }

    // every tile is in a column with the tiles next to it, and a column is as many tiles as the
    // strip says
    if layout.len() != space.tile_count() || ids.len() != space.tile_count() {
        return Err("the number of tiles is not the number of tiles".into());
    }

    Ok(())
}

fn run(seed: u64) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9).wrapping_add(1));

    let gaps = rng.pick(&[0, 8, 16]);
    let options = Options {
        gaps,
        default_width: random_width(&mut rng),
        center_focused_column: rng.pick(&[
            CenterFocusedColumn::Never,
            CenterFocusedColumn::Always,
            CenterFocusedColumn::OnOverflow,
        ]),
        always_center_single_column: rng.below(2) == 0,
        ..Options::default()
    };
    let mut space = ScrollingSpace::new(
        options.clone(),
        Viewport {
            width: 1200,
            height: 800,
        },
    );

    let mut next_id = 0;
    let mut log = vec![];

    for index in 0..STEPS {
        let before = Before::of(&space);
        let promise = step(&mut rng, &mut space, &mut next_id, &mut log);

        if let Err(why) = check(&space, &before, &promise, gaps) {
            let steps: Vec<String> = log
                .iter()
                .enumerate()
                .map(|(i, step)| format!("  {i:>3}: {step}"))
                .collect();

            panic!(
                "seed {seed}, after step {index}: {why}\noptions: {options:?}\nsteps:\n{}\nlayout: {:#?}",
                steps.join("\n"),
                space.layout()
            );
        }
    }
}

#[test]
fn whatever_is_done_the_strip_stays_sane_and_the_focused_column_stays_put() {
    for seed in 0..SEEDS {
        run(seed);
    }
}
