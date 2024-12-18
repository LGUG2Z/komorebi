use eframe::egui::CursorIcon;
use eframe::egui::Frame;
use eframe::egui::Margin;
use eframe::egui::Response;
use eframe::egui::Sense;
use eframe::egui::Ui;

/// Same as SelectableLabel, but supports all content
pub struct SelectableFrame {
    selected: bool,
}

impl SelectableFrame {
    pub fn new(selected: bool) -> Self {
        Self { selected }
    }

    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> Response {
        let Self { selected } = self;

        Frame::none()
            .show(ui, |ui| {
                let response = ui.interact(ui.max_rect(), ui.unique_id(), Sense::click());

                if ui.is_rect_visible(response.rect) {
                    let inner_margin = Margin::symmetric(
                        ui.style().spacing.button_padding.x,
                        ui.style().spacing.button_padding.y,
                    );

                    if selected
                        || response.hovered()
                        || response.highlighted()
                        || response.has_focus()
                    {
                        let visuals = ui.style().interact_selectable(&response, selected);

                        Frame::none()
                            .stroke(visuals.bg_stroke)
                            .rounding(visuals.rounding)
                            .fill(visuals.bg_fill)
                            .inner_margin(inner_margin)
                            .show(ui, add_contents);
                    } else {
                        Frame::none()
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
