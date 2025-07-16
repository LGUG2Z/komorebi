use eframe::egui::ViewportBuilder;
use std::path::PathBuf;
use whkd_core::Whkdrc;

#[derive(Default)]
struct Quicklook {
    whkdrc: Option<Whkdrc>,
    filter: String,
}

impl Quicklook {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        let mut home = std::env::var("WHKD_CONFIG_HOME").map_or_else(
            |_| {
                dirs::home_dir()
                    .expect("no home directory found")
                    .join(".config")
            },
            |home_path| {
                let home = PathBuf::from(&home_path);

                if home.as_path().is_dir() {
                    home
                } else {
                    panic!(
                        "$Env:WHKD_CONFIG_HOME is set to '{home_path}', which is not a valid directory",
                    );
                }
            },
        );
        home.push("whkdrc");

        Self {
            whkdrc: whkd_parser::load(&home).ok(),
            filter: String::new(),
        }
    }
}

impl eframe::App for Quicklook {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_max_width(ui.available_width());
            ui.set_max_height(ui.available_height());
            eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                eframe::egui::Grid::new("grid")
                    .num_columns(2)
                    .striped(true)
                    .spacing([40.0, 4.0])
                    .min_col_width(ui.available_width() / 2.0 - 20.0)
                    .show(ui, |ui| {
                        if let Some(whkdrc) = &self.whkdrc {
                            ui.label("Filter");
                            ui.add(
                                eframe::egui::text_edit::TextEdit::singleline(&mut self.filter)
                                    .hint_text("Filter by command...")
                                    .background_color(ctx.style().visuals.faint_bg_color),
                            );
                            ui.end_row();

                            for binding in &whkdrc.bindings {
                                let keys = binding.keys.join(" + ");
                                if self.filter.is_empty() || binding.command.contains(&self.filter)
                                {
                                    ui.label(keys);
                                    ui.label(&binding.command);
                                    ui.end_row();
                                }
                            }
                        }
                    });
            });
        });
    }
}

fn main() {
    let viewport_builder = ViewportBuilder::default()
        .with_resizable(true)
        .with_decorations(false);

    let native_options = eframe::NativeOptions {
        viewport: viewport_builder,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "komorebi-shortcuts",
        native_options,
        Box::new(|cc| Ok(Box::new(Quicklook::new(cc)))),
    )
    .unwrap();
}
