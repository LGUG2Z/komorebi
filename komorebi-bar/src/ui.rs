use eframe::egui::Align;
use eframe::egui::Layout;
use eframe::egui::Response;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use eframe::egui::Widget;

pub struct CustomUi<'ui>(pub &'ui mut Ui);

impl CustomUi<'_> {
    pub fn add_sized_left_to_right(
        &mut self,
        max_size: impl Into<Vec2>,
        widget: impl Widget,
    ) -> Response {
        let layout = Layout::left_to_right(Align::Center);
        self.0
            .allocate_ui_with_layout(max_size.into(), layout, |ui| ui.add(widget))
            .inner
    }
}
