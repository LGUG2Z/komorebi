//! The pictures: each display's frame at the playhead, with komorebi's idea of the layout drawn
//! over it if asked.
//!
//! [`Open::video_ui`] draws one picture for each display that has video, side by side
//! ([`arrange`]), and, with the overlay on, komorebi's state over them. What is drawn is described
//! on [`Open::video_ui`], whose documentation is public. [`arrange`] and [`on_tile`] are plain
//! arithmetic, covered by the unit tests in this module
//! (`one_picture_fills_the_space_it_fits_and_is_centred`,
//! `several_pictures_sit_side_by_side_at_one_height`, `nothing_to_arrange_is_nothing` and
//! `global_points_map_onto_the_picture_of_their_display`); the module is not public, so they have
//! no examples. Drawing needs an `egui::Context`.

use super::app::Open;
use super::app::Tile;
use super::theme;
use crate::analysis::state::Snapshot;
use crate::playback::DisplayView;
use eframe::egui;
use eframe::egui::Color32;
use eframe::egui::Pos2;
use eframe::egui::Rect;
use eframe::egui::Stroke;
use eframe::egui::StrokeKind;
use eframe::egui::vec2;
use std::sync::Arc;

/// Returns how the pictures of several displays share the space: side by side, each as tall as
/// fits and as wide as its shape needs. There is one rectangle for each of `aspects`, in order.
///
/// `aspects` are the width over the height of each picture and `gap` is the space between
/// neighboring pictures. All the pictures have one height: the largest at which the row, gaps
/// included, fits the width of `available` and the picture fits its height (and at least one
/// point). The row is centered in `available`, both ways. No pictures give no rectangles.
#[must_use]
pub fn arrange(available: Rect, aspects: &[f32], gap: f32) -> Vec<Rect> {
    if aspects.is_empty() {
        return Vec::new();
    }

    let total_aspect: f32 = aspects.iter().sum();
    let gaps = gap * (aspects.len() as f32 - 1.0);
    let height = ((available.width() - gaps) / total_aspect)
        .min(available.height())
        .max(1.0);
    let used = total_aspect * height + gaps;

    let mut x = available.left() + (available.width() - used) / 2.0;
    let top = available.top() + (available.height() - height) / 2.0;
    aspects
        .iter()
        .map(|aspect| {
            let rect = Rect::from_min_size(Pos2::new(x, top), vec2(aspect * height, height));
            x += aspect * height + gap;
            rect
        })
        .collect()
}

/// Returns where a rectangle in global points falls on a picture of the display whose frame is
/// `frame` (`[x, y, width, height]` in the same points), drawn into `tile`.
///
/// The rectangle is `left`, `top`, `width` and `height` in global points, as komorebi reports
/// windows and layouts. The result is in the coordinates of `tile`: the display's top-left corner
/// is the tile's, and a rectangle half the display's width is half the tile's. It is not clipped,
/// so a window partly or wholly off the display gives a rectangle that sticks out of the tile. A
/// frame with a width or height under one point is taken as one point.
#[must_use]
pub fn on_tile(tile: Rect, frame: [f64; 4], left: i64, top: i64, width: i64, height: i64) -> Rect {
    let [x, y, frame_width, frame_height] = frame;
    let scale_x = f64::from(tile.width()) / frame_width.max(1.0);
    let scale_y = f64::from(tile.height()) / frame_height.max(1.0);
    Rect::from_min_size(
        Pos2::new(
            tile.left() + ((left as f64 - x) * scale_x) as f32,
            tile.top() + ((top as f64 - y) * scale_y) as f32,
        ),
        vec2(
            (width as f64 * scale_x) as f32,
            (height as f64 * scale_y) as f32,
        ),
    )
}

impl Open {
    /// Draws the displays into `ui`, which is the middle of the window.
    ///
    /// One picture is drawn for each display that has video, side by side and each as tall as fits.
    /// Each has a label (`display N`, and, when it shows a frame, when that frame was on screen) and
    /// is outlined more heavily if it is the focused display ([`Open::focus`]). Clicking a picture
    /// focuses its display. A recording with no video at all shows a note to that effect instead.
    ///
    /// What a picture shows comes from [`Playback::view`](crate::playback::Playback::view):
    ///
    /// | [`DisplayView`] | drawn as |
    /// |---|---|
    /// | `Frame` | the frame, uploaded to the graphics card once for each new frame |
    /// | `Gap` | a placeholder saying the display has no video there and, when the recording shows why, the reason |
    /// | `Loading` | "decoding…" |
    /// | `Failed` | "could not decode:" and the reason |
    ///
    /// A gap says why from the machine's incidents when it can: the last display-disconnected,
    /// display-asleep, capture-stopped, capture-failed or display-reconfigured incident for that
    /// display, up to half a second ahead of the playhead; failing that, when the display's next
    /// video begins; failing that, "unplugged, asleep, or not yet recorded".
    ///
    /// # The state overlay
    ///
    /// With the overlay on ([`Open::overlay`]: the "state overlay" box, and `O`), komorebi's state at
    /// the playhead ([`Open::state_at`]) is drawn over each picture that shows a frame. A dashed
    /// rectangle is where komorebi meant each tiled window of the display's focused workspace to
    /// be, and a solid one is where each visible window really was. A solid rectangle is red if the
    /// window breaks an invariant of the state ([`crate::analysis::invariants::check`]), the accent
    /// color if it has focus and green otherwise. Windows komorebi has parked out of sight are not
    /// drawn. A display is matched to komorebi's monitor of the same id, and positions are in the
    /// global points komorebi uses.
    ///
    /// `ctx` is used to upload frames as textures. Needs an `egui::Context`, so a window is needed.
    pub fn video_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.loaded.displays.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("this recording has no video")
                        .color(theme::TEXT_DIM)
                        .size(16.0),
                );
            });
            return;
        }

        let ids: Vec<u32> = self
            .loaded
            .displays
            .iter()
            .map(|display| display.id)
            .collect();
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, egui::Sense::hover());

        // the shape of each picture: from the frame on screen, else from the display
        let aspects: Vec<f32> = ids
            .iter()
            .map(|id| match self.playback.view(*id) {
                DisplayView::Frame(frame) if frame.height > 0 => {
                    frame.width as f32 / frame.height as f32
                }
                _ => {
                    let frame = self
                        .loaded
                        .displays
                        .iter()
                        .find(|d| d.id == *id)
                        .map_or([0.0, 0.0, 16.0, 9.0], |d| d.frame);
                    (frame[2] / frame[3].max(1.0)) as f32
                }
            })
            .collect();

        let snapshot = if self.overlay {
            self.state_at(self.position())
        } else {
            None
        };

        let rects = arrange(area, &aspects, 10.0);
        for (id, rect) in ids.iter().zip(rects) {
            let view = self.playback.view(*id);
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, Color32::BLACK);

            match &view {
                DisplayView::Frame(frame) => {
                    let pointer = Arc::as_ptr(frame) as usize;
                    let stale = self.tiles.get(id).is_none_or(|tile| tile.frame != pointer);
                    if stale {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [frame.width as usize, frame.height as usize],
                            &frame.rgba,
                        );
                        match self.tiles.get_mut(id) {
                            Some(tile) => {
                                tile.texture.set(image, egui::TextureOptions::LINEAR);
                                tile.frame = pointer;
                            }
                            None => {
                                let texture = ctx.load_texture(
                                    format!("display-{id}"),
                                    image,
                                    egui::TextureOptions::LINEAR,
                                );
                                self.tiles.insert(
                                    *id,
                                    Tile {
                                        texture,
                                        frame: pointer,
                                    },
                                );
                            }
                        }
                    }

                    if let Some(tile) = self.tiles.get(id) {
                        painter.image(
                            tile.texture.id(),
                            rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                }
                DisplayView::Gap => self.gap_placeholder(&painter, rect, *id),
                DisplayView::Loading => {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "decoding…",
                        egui::FontId::proportional(14.0),
                        theme::TEXT_DIM,
                    );
                }
                DisplayView::Failed(message) => {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("could not decode: {message}"),
                        egui::FontId::proportional(13.0),
                        theme::BUG,
                    );
                }
            }

            if let Some(snapshot) = &snapshot
                && matches!(view, DisplayView::Frame(_))
                && let Some(meta) = self.loaded.displays.iter().find(|meta| meta.id == *id)
            {
                self.draw_overlay(&painter, rect, meta.frame, *id, snapshot);
            }

            // which picture this is, and which one the keys act on
            let focused = self.focus == Some(*id);
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(
                    if focused { 2.0 } else { 1.0_f32 },
                    if focused {
                        theme::ACCENT
                    } else {
                        Color32::from_gray(60)
                    },
                ),
                StrokeKind::Inside,
            );
            let shown = match &view {
                DisplayView::Frame(frame) => {
                    format!("  ·  frame from {}", crate::timecode::format(frame.shown_s))
                }
                _ => String::new(),
            };
            painter.text(
                rect.left_bottom() + vec2(8.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                format!("display {id}{shown}"),
                egui::FontId::monospace(11.0),
                Color32::from_rgba_unmultiplied(255, 255, 255, 200),
            );

            let response = ui.interact(rect, ui.id().with(("tile", *id)), egui::Sense::click());
            if response.clicked() {
                self.focus = Some(*id);
            }
        }
    }

    /// What to show where a display has no video: why, if the recording says.
    fn gap_placeholder(&self, painter: &egui::Painter, rect: Rect, display: u32) {
        painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 18, 22));
        let now = self.position();
        // why it has none: what happened to this display most recently, or that it had not begun
        let ended = self
            .loaded
            .incidents
            .iter()
            .rfind(|incident| {
                incident.display == Some(display)
                    && incident.video_s <= now + 0.5
                    && matches!(
                        incident.kind.as_str(),
                        "display-disconnected"
                            | "display-asleep"
                            | "capture-stopped"
                            | "capture-failed"
                            | "display-reconfigured"
                    )
            })
            .map(|incident| format!("{}: {}", incident.kind, incident.detail));
        let begins = self
            .loaded
            .displays
            .iter()
            .find(|meta| meta.id == display)
            .and_then(|meta| {
                meta.segments
                    .iter()
                    .map(|(from, _)| *from)
                    .find(|from| *from > now)
            })
            .map(|from| format!("its video begins at {}", crate::timecode::format(from)));
        let why = ended.or(begins);

        painter.text(
            rect.center() - vec2(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            format!("display {display} has no video here"),
            egui::FontId::proportional(15.0),
            theme::TEXT_DIM,
        );
        painter.text(
            rect.center() + vec2(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            why.unwrap_or_else(|| "unplugged, asleep, or not yet recorded".to_string()),
            egui::FontId::proportional(12.0),
            Color32::from_gray(110),
        );
    }

    /// komorebi's idea of the layout, over the display it is about: where it meant each tiled
    /// window to be (dashed) and where the window really was (solid).
    fn draw_overlay(
        &self,
        painter: &egui::Painter,
        tile: Rect,
        frame: [f64; 4],
        display: u32,
        snapshot: &Snapshot,
    ) {
        let Some(monitor) = snapshot
            .monitors
            .iter()
            .find(|monitor| u32::try_from(monitor.id).ok() == Some(display))
        else {
            return;
        };
        let Some(workspace) = monitor.focused_workspace() else {
            return;
        };

        let focus = snapshot.focus();
        let focused_window = focus.as_ref().and_then(|focus| focus.window_id);
        let violating: Vec<u64> = crate::analysis::invariants::check(snapshot)
            .into_iter()
            .flat_map(|violation| violation.window_ids)
            .collect();

        for rect in &workspace.layout_rects {
            let drawn = on_tile(tile, frame, rect.left, rect.top, rect.width, rect.height);
            painter.add(egui::Shape::dashed_line(
                &[
                    drawn.left_top(),
                    drawn.right_top(),
                    drawn.right_bottom(),
                    drawn.left_bottom(),
                    drawn.left_top(),
                ],
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(240, 200, 80, 190)),
                5.0,
                4.0,
            ));
        }

        for (location, window) in snapshot.windows() {
            if location.monitor_id != monitor.id || !location.workspace_visible {
                continue;
            }
            let Some(rect) = &window.rect else {
                continue;
            };
            if snapshot.is_hidden_position(rect, monitor) {
                continue;
            }

            let drawn = on_tile(tile, frame, rect.left, rect.top, rect.width, rect.height);
            let wrong = violating.contains(&window.id);
            let is_focused = focused_window == Some(window.id);
            let colour = if wrong {
                theme::BUG
            } else if is_focused {
                theme::ACCENT
            } else {
                Color32::from_rgba_unmultiplied(110, 220, 150, 200)
            };
            painter.rect_stroke(
                drawn,
                0.0,
                Stroke::new(if wrong || is_focused { 2.0 } else { 1.0_f32 }, colour),
                StrokeKind::Inside,
            );
            painter.text(
                drawn.left_top() + vec2(4.0, 3.0),
                egui::Align2::LEFT_TOP,
                format!("{} {}", window.exe, window.id),
                egui::FontId::monospace(10.0),
                colour,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(width: f32, height: f32) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(width, height))
    }

    #[test]
    fn one_picture_fills_the_space_it_fits_and_is_centred() {
        let rects = arrange(rect(1000.0, 400.0), &[2.0], 10.0);
        assert_eq!(rects.len(), 1);
        // 400 tall at 2:1 is 800 wide, centred in 1000
        assert!(
            (rects[0].width() - 800.0).abs() < 0.01 && (rects[0].height() - 400.0).abs() < 0.01
        );
        assert!((rects[0].left() - 100.0).abs() < 0.01);
    }

    #[test]
    fn several_pictures_sit_side_by_side_at_one_height() {
        let rects = arrange(rect(1200.0, 1000.0), &[1.5, 2.5], 20.0);
        assert_eq!(rects.len(), 2);
        assert!((rects[0].height() - rects[1].height()).abs() < 0.01);
        assert!((rects[1].left() - rects[0].right() - 20.0).abs() < 0.01);
        // wide screens are wide: the whole row uses the width (1180 / 4)
        assert!((rects[0].height() - 295.0).abs() < 0.01);
        assert!(rects[1].right() <= 1200.0 + 0.01);
    }

    #[test]
    fn nothing_to_arrange_is_nothing() {
        assert!(arrange(rect(100.0, 100.0), &[], 10.0).is_empty());
    }

    #[test]
    fn global_points_map_onto_the_picture_of_their_display() {
        // a display at (1000, 0), 2000 x 1000 points, drawn into a 400 x 200 tile at (50, 30)
        let tile = Rect::from_min_size(Pos2::new(50.0, 30.0), vec2(400.0, 200.0));
        let frame = [1000.0, 0.0, 2000.0, 1000.0];

        // its top-left corner is the tile's
        let corner = on_tile(tile, frame, 1000, 0, 0, 0);
        assert!((corner.left() - 50.0).abs() < 0.01 && (corner.top() - 30.0).abs() < 0.01);

        // a window 500 x 250 at (2000, 500) is a fifth of the tile's size, in the middle
        let window = on_tile(tile, frame, 2000, 500, 500, 250);
        assert!((window.left() - 250.0).abs() < 0.01 && (window.top() - 130.0).abs() < 0.01);
        assert!((window.width() - 100.0).abs() < 0.01 && (window.height() - 50.0).abs() < 0.01);
    }
}
