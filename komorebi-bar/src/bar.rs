use crate::config::KomobarConfig;
use crate::config::Theme;
use crate::komorebi::Komorebi;
use crate::komorebi::KomorebiNotificationState;
use crate::widget::BarWidget;
use crate::widget::WidgetConfig;
use crossbeam_channel::Receiver;
use eframe::egui::Align;
use eframe::egui::CentralPanel;
use eframe::egui::Context;
use eframe::egui::FontData;
use eframe::egui::FontDefinitions;
use eframe::egui::FontFamily;
use eframe::egui::Frame;
use eframe::egui::Layout;
use eframe::egui::Margin;
use font_loader::system_fonts;
use font_loader::system_fonts::FontPropertyBuilder;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

pub struct Komobar {
    pub config: KomobarConfig,
    pub komorebi_notification_state: Option<Rc<RefCell<KomorebiNotificationState>>>,
    pub left_widgets: Vec<Box<dyn BarWidget>>,
    pub right_widgets: Vec<Box<dyn BarWidget>>,
    pub rx_gui: Receiver<komorebi_client::Notification>,
}

impl Komobar {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rx_gui: Receiver<komorebi_client::Notification>,
        config: Arc<KomobarConfig>,
    ) -> Self {
        if let Some(font_family) = &config.font_family {
            Self::add_custom_font(&cc.egui_ctx, font_family);
        }

        match config.theme {
            Some(Theme::CatppuccinFrappe) => {
                catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::FRAPPE);
            }
            Some(Theme::CatppuccinMacchiato) => {
                catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MACCHIATO);
            }
            Some(Theme::CatppuccinMocha) => {
                catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MOCHA);
            }
            Some(Theme::Default) | None => {}
        }

        let mut komorebi_widget = None;
        let mut komorebi_widget_idx = None;
        let mut komorebi_notification_state = None;
        let mut side = None;

        for (idx, widget_config) in config.left_widgets.iter().enumerate() {
            if let WidgetConfig::Komorebi(config) = widget_config {
                komorebi_widget = Some(Komorebi::from(*config));
                komorebi_widget_idx = Some(idx);
                side = Some(Side::Left);
            }
        }

        for (idx, widget_config) in config.right_widgets.iter().enumerate() {
            if let WidgetConfig::Komorebi(config) = widget_config {
                komorebi_widget = Some(Komorebi::from(*config));
                komorebi_widget_idx = Some(idx);
                side = Some(Side::Right);
            }
        }

        let mut left_widgets = config
            .left_widgets
            .iter()
            .map(|config| config.as_boxed_bar_widget())
            .collect::<Vec<Box<dyn BarWidget>>>();

        let mut right_widgets = config
            .right_widgets
            .iter()
            .map(|config| config.as_boxed_bar_widget())
            .collect::<Vec<Box<dyn BarWidget>>>();

        if let (Some(idx), Some(widget), Some(side)) = (komorebi_widget_idx, komorebi_widget, side)
        {
            komorebi_notification_state = Some(widget.komorebi_notification_state.clone());
            let boxed: Box<dyn BarWidget> = Box::new(widget);
            match side {
                Side::Left => left_widgets[idx] = boxed,
                Side::Right => right_widgets[idx] = boxed,
            }
        }

        right_widgets.reverse();

        Self {
            config: config.deref().clone(),
            komorebi_notification_state,
            left_widgets,
            right_widgets,
            rx_gui,
        }
    }

    fn add_custom_font(ctx: &Context, name: &str) {
        let mut fonts = FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

        let property = FontPropertyBuilder::new().family(name).build();

        if let Some((font, _)) = system_fonts::get(&property) {
            fonts
                .font_data
                .insert(name.to_owned(), FontData::from_owned(font));

            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, name.to_owned());

            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .push(name.to_owned());

            // Tell egui to use these fonts:
            ctx.set_fonts(fonts);
        }
    }
}
impl eframe::App for Komobar {
    // TODO: I think this is needed for transparency??
    // fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
    // egui::Rgba::TRANSPARENT.to_array()
    // let mut background = Color32::from_gray(18).to_normalized_gamma_f32();
    // background[3] = 0.9;
    // background
    // }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if let Some(komorebi_notification_state) = &self.komorebi_notification_state {
            komorebi_notification_state
                .borrow_mut()
                .handle_notification(self.config.monitor.index, self.rx_gui.clone());
        }

        let frame = if let Some(frame) = &self.config.frame {
            Frame::none().outer_margin(Margin::symmetric(
                frame.outer_margin.x,
                frame.outer_margin.y,
            ))
        } else {
            Frame::none()
        };

        CentralPanel::default().frame(frame).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    for w in &mut self.left_widgets {
                        w.render(ctx, ui);
                    }
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    for w in &mut self.right_widgets {
                        w.render(ctx, ui);
                    }
                })
            })
        });
    }
}

#[derive(Copy, Clone)]
enum Side {
    Left,
    Right,
}
