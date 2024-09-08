use eframe::egui::Context;
use eframe::egui::Ui;

pub trait BarWidget {
    fn render(&mut self, ctx: &Context, ui: &mut Ui);
}
