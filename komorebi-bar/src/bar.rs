use crate::config::KomobarConfig;
use crate::config::KomobarTheme;
use crate::komorebi::Komorebi;
use crate::komorebi::KomorebiNotificationState;
use crate::widget::BarWidget;
use crate::widget::WidgetConfig;
use crossbeam_channel::Receiver;
use eframe::egui::Align;
use eframe::egui::CentralPanel;
use eframe::egui::Color32;
use eframe::egui::Context;
use eframe::egui::FontData;
use eframe::egui::FontDefinitions;
use eframe::egui::FontFamily;
use eframe::egui::FontId;
use eframe::egui::Frame;
use eframe::egui::Layout;
use eframe::egui::Margin;
use eframe::egui::Style;
use eframe::egui::TextStyle;
use eframe::egui::Vec2;
use eframe::egui::ViewportCommand;
use font_loader::system_fonts;
use font_loader::system_fonts::FontPropertyBuilder;
use komorebi_themes::catppuccin_egui;
use komorebi_themes::Catppuccin;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub struct Komobar {
    pub config: Arc<KomobarConfig>,
    pub komorebi_notification_state: Option<Rc<RefCell<KomorebiNotificationState>>>,
    pub left_widgets: Vec<Box<dyn BarWidget>>,
    pub right_widgets: Vec<Box<dyn BarWidget>>,
    pub rx_gui: Receiver<komorebi_client::Notification>,
    pub rx_config: Receiver<KomobarConfig>,
    pub bg_color: Rc<RefCell<Color32>>,
    pub scale_factor: f32,
}

pub fn apply_theme(ctx: &Context, theme: KomobarTheme, bg_color: Rc<RefCell<Color32>>) {
    match theme {
        KomobarTheme::Catppuccin {
            name: catppuccin,
            accent: catppuccin_value,
        } => match catppuccin {
            Catppuccin::Frappe => {
                catppuccin_egui::set_theme(ctx, catppuccin_egui::FRAPPE);
                let catppuccin_value = catppuccin_value.unwrap_or_default();
                let accent = catppuccin_value.color32(catppuccin.as_theme());

                ctx.style_mut(|style| {
                    style.visuals.selection.stroke.color = accent;
                    style.visuals.widgets.hovered.fg_stroke.color = accent;
                    style.visuals.widgets.active.fg_stroke.color = accent;
                    style.visuals.override_text_color = None;
                });

                bg_color.replace(catppuccin_egui::FRAPPE.base);
            }
            Catppuccin::Latte => {
                catppuccin_egui::set_theme(ctx, catppuccin_egui::LATTE);
                let catppuccin_value = catppuccin_value.unwrap_or_default();
                let accent = catppuccin_value.color32(catppuccin.as_theme());

                ctx.style_mut(|style| {
                    style.visuals.selection.stroke.color = accent;
                    style.visuals.widgets.hovered.fg_stroke.color = accent;
                    style.visuals.widgets.active.fg_stroke.color = accent;
                    style.visuals.override_text_color = None;
                });

                bg_color.replace(catppuccin_egui::LATTE.base);
            }
            Catppuccin::Macchiato => {
                catppuccin_egui::set_theme(ctx, catppuccin_egui::MACCHIATO);
                let catppuccin_value = catppuccin_value.unwrap_or_default();
                let accent = catppuccin_value.color32(catppuccin.as_theme());

                ctx.style_mut(|style| {
                    style.visuals.selection.stroke.color = accent;
                    style.visuals.widgets.hovered.fg_stroke.color = accent;
                    style.visuals.widgets.active.fg_stroke.color = accent;
                    style.visuals.override_text_color = None;
                });

                bg_color.replace(catppuccin_egui::MACCHIATO.base);
            }
            Catppuccin::Mocha => {
                catppuccin_egui::set_theme(ctx, catppuccin_egui::MOCHA);
                let catppuccin_value = catppuccin_value.unwrap_or_default();
                let accent = catppuccin_value.color32(catppuccin.as_theme());

                ctx.style_mut(|style| {
                    style.visuals.selection.stroke.color = accent;
                    style.visuals.widgets.hovered.fg_stroke.color = accent;
                    style.visuals.widgets.active.fg_stroke.color = accent;
                    style.visuals.override_text_color = None;
                });

                bg_color.replace(catppuccin_egui::MOCHA.base);
            }
        },
        KomobarTheme::Base16 {
            name: base16,
            accent: base16_value,
        } => {
            ctx.set_style(base16.style());
            let base16_value = base16_value.unwrap_or_default();
            let accent = base16_value.color32(base16);

            ctx.style_mut(|style| {
                style.visuals.selection.stroke.color = accent;
                style.visuals.widgets.hovered.fg_stroke.color = accent;
                style.visuals.widgets.active.fg_stroke.color = accent;
            });

            bg_color.replace(base16.background());
        }
    }
}

impl Komobar {
    pub fn apply_config(
        &mut self,
        ctx: &Context,
        config: &KomobarConfig,
        previous_notification_state: Option<Rc<RefCell<KomorebiNotificationState>>>,
    ) {
        if let Some(font_family) = &config.font_family {
            tracing::info!("attempting to add custom font family: {font_family}");
            Self::add_custom_font(ctx, font_family);
        }

        if let Some(viewport) = &config.viewport {
            if let Some(inner_size) = viewport.inner_size {
                let mut vec2 = Vec2::new(inner_size.x, inner_size.y * 2.0);
                if self.scale_factor != 1.0 {
                    vec2 = Vec2::new(inner_size.x / self.scale_factor, inner_size.y * 2.0);
                }

                ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2));
            }
        }

        match config.theme {
            Some(theme) => {
                apply_theme(ctx, theme, self.bg_color.clone());
            }
            None => {
                let home_dir: PathBuf = std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(
                    |_| dirs::home_dir().expect("there is no home directory"),
                    |home_path| {
                        let home = PathBuf::from(&home_path);

                        if home.as_path().is_dir() {
                            home
                        } else {
                            panic!("$Env:KOMOREBI_CONFIG_HOME is set to '{home_path}', which is not a valid directory");
                        }
                    },
                );

                let config = home_dir.join("komorebi.json");
                match komorebi_client::StaticConfig::read(&config) {
                    Ok(config) => {
                        if let Some(theme) = config.theme {
                            apply_theme(ctx, KomobarTheme::from(theme), self.bg_color.clone());
                        }
                    }
                    Err(_) => {
                        ctx.set_style(Style::default());
                        self.bg_color.replace(Style::default().visuals.panel_fill);
                    }
                }
            }
        }

        if let Some(font_size) = &config.font_size {
            tracing::info!("attempting to set custom font size: {font_size}");
            Self::set_font_size(ctx, *font_size);
        }

        let mut komorebi_widget = None;
        let mut komorebi_widget_idx = None;
        let mut komorebi_notification_state = previous_notification_state;
        let mut side = None;

        for (idx, widget_config) in config.left_widgets.iter().enumerate() {
            if let WidgetConfig::Komorebi(config) = widget_config {
                komorebi_widget = Some(Komorebi::from(config));
                komorebi_widget_idx = Some(idx);
                side = Some(Side::Left);
            }
        }

        for (idx, widget_config) in config.right_widgets.iter().enumerate() {
            if let WidgetConfig::Komorebi(config) = widget_config {
                komorebi_widget = Some(Komorebi::from(config));
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

        if let (Some(idx), Some(mut widget), Some(side)) =
            (komorebi_widget_idx, komorebi_widget, side)
        {
            match komorebi_notification_state {
                None => {
                    komorebi_notification_state = Some(widget.komorebi_notification_state.clone());
                }
                Some(ref previous) => {
                    previous
                        .borrow_mut()
                        .update_from_config(&widget.komorebi_notification_state.borrow());

                    widget.komorebi_notification_state = previous.clone();
                }
            }

            let boxed: Box<dyn BarWidget> = Box::new(widget);
            match side {
                Side::Left => left_widgets[idx] = boxed,
                Side::Right => right_widgets[idx] = boxed,
            }
        }

        right_widgets.reverse();

        self.left_widgets = left_widgets;
        self.right_widgets = right_widgets;

        tracing::info!("widget configuration options applied");

        self.komorebi_notification_state = komorebi_notification_state;
    }
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rx_gui: Receiver<komorebi_client::Notification>,
        rx_config: Receiver<KomobarConfig>,
        config: Arc<KomobarConfig>,
    ) -> Self {
        let mut komobar = Self {
            config: config.clone(),
            komorebi_notification_state: None,
            left_widgets: vec![],
            right_widgets: vec![],
            rx_gui,
            rx_config,
            bg_color: Rc::new(RefCell::new(Style::default().visuals.panel_fill)),
            scale_factor: cc.egui_ctx.native_pixels_per_point().unwrap_or(1.0),
        };

        komobar.apply_config(&cc.egui_ctx, &config, None);

        komobar
    }

    fn set_font_size(ctx: &Context, font_size: f32) {
        ctx.style_mut(|style| {
            style.text_styles = [
                (TextStyle::Small, FontId::new(9.0, FontFamily::Proportional)),
                (
                    TextStyle::Body,
                    FontId::new(font_size, FontFamily::Proportional),
                ),
                (
                    TextStyle::Button,
                    FontId::new(font_size, FontFamily::Proportional),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(18.0, FontFamily::Proportional),
                ),
                (
                    TextStyle::Monospace,
                    FontId::new(font_size, FontFamily::Monospace),
                ),
            ]
            .into();
        });
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
        if self.scale_factor != ctx.native_pixels_per_point().unwrap_or(1.0) {
            self.scale_factor = ctx.native_pixels_per_point().unwrap_or(1.0);
            self.apply_config(
                ctx,
                &self.config.clone(),
                self.komorebi_notification_state.clone(),
            );
        }

        if let Ok(updated_config) = self.rx_config.try_recv() {
            self.apply_config(
                ctx,
                &updated_config,
                self.komorebi_notification_state.clone(),
            );
        }

        if let Some(komorebi_notification_state) = &self.komorebi_notification_state {
            komorebi_notification_state
                .borrow_mut()
                .handle_notification(
                    ctx,
                    self.config.monitor.index,
                    self.rx_gui.clone(),
                    self.bg_color.clone(),
                );
        }

        let frame = if let Some(frame) = &self.config.frame {
            Frame::none()
                .inner_margin(Margin::symmetric(
                    frame.inner_margin.x,
                    frame.inner_margin.y,
                ))
                .fill(*self.bg_color.borrow())
        } else {
            Frame::none().fill(*self.bg_color.borrow())
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
