use eframe::egui::Ui;

pub trait BarWidget {
    fn output(&mut self) -> Vec<String>;
    fn render(&mut self, ui: &mut Ui);
}
