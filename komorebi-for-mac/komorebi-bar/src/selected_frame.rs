use eframe::egui::Color32;
use eframe::egui::CursorIcon;
use eframe::egui::Frame;
use eframe::egui::Margin;
use eframe::egui::Response;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::Ui;

/// Same as SelectableLabel, but supports all content
pub struct SelectableFrame {
    selected: bool,
    selected_fill: Option<Color32>,
}

impl SelectableFrame {
    pub fn new(selected: bool) -> Self {
        Self {
            selected,
            selected_fill: None,
        }
    }

    pub fn new_auto(selected: bool, selected_fill: Option<Color32>) -> Self {
        Self {
            selected,
            selected_fill,
        }
    }

    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> Response {
        let Self {
            selected,
            selected_fill,
        } = self;

        Frame::NONE
            .show(ui, |ui| {
                let response = ui.interact(ui.max_rect(), ui.unique_id(), Sense::click());

                if ui.is_rect_visible(response.rect) {
                    // take into account the stroke width
                    let inner_margin = Margin::symmetric(
                        ui.style().spacing.button_padding.x as i8 - 1,
                        ui.style().spacing.button_padding.y as i8 - 1,
                    );

                    // since the stroke is drawn inside the frame, we always reserve space for it
                    if selected && response.hovered() {
                        let visuals = ui.style().interact_selectable(&response, selected);

                        Frame::NONE
                            .stroke(Stroke::new(1.0, visuals.bg_stroke.color))
                            .corner_radius(visuals.corner_radius)
                            .fill(selected_fill.unwrap_or(visuals.bg_fill))
                            .inner_margin(inner_margin)
                            .show(ui, add_contents);
                    } else if response.hovered() || response.highlighted() || response.has_focus() {
                        let visuals = ui.style().interact_selectable(&response, selected);

                        Frame::NONE
                            .stroke(Stroke::new(1.0, visuals.bg_stroke.color))
                            .corner_radius(visuals.corner_radius)
                            .fill(visuals.bg_fill)
                            .inner_margin(inner_margin)
                            .show(ui, add_contents);
                    } else if selected {
                        let visuals = ui.style().interact_selectable(&response, selected);

                        Frame::NONE
                            .stroke(Stroke::new(1.0, visuals.bg_fill))
                            .corner_radius(visuals.corner_radius)
                            .fill(selected_fill.unwrap_or(visuals.bg_fill))
                            .inner_margin(inner_margin)
                            .show(ui, add_contents);
                    } else {
                        Frame::NONE
                            .stroke(Stroke::new(1.0, Color32::TRANSPARENT))
                            .inner_margin(inner_margin)
                            .show(ui, add_contents);
                    }
                }

                response
            })
            .inner
            .on_hover_cursor(CursorIcon::PointingHand)
    }
}
