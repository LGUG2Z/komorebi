use crate::config::KomobarConfig;
use crate::config::KomobarTheme;
use crate::config::Position;
use crate::config::PositionConfig;
use crate::komorebi::Komorebi;
use crate::komorebi::KomorebiNotificationState;
use crate::process_hwnd;
use crate::render::Color32Ext;
use crate::render::Grouping;
use crate::render::RenderConfig;
use crate::render::RenderExt;
use crate::widget::BarWidget;
use crate::widget::WidgetConfig;
use crate::BAR_HEIGHT;
use crate::MAX_LABEL_WIDTH;
use crate::MONITOR_LEFT;
use crate::MONITOR_RIGHT;
use crate::MONITOR_TOP;
use crossbeam_channel::Receiver;
use eframe::egui::Align2;
use eframe::egui::Area;
use eframe::egui::CentralPanel;
use eframe::egui::Color32;
use eframe::egui::Context;
use eframe::egui::FontData;
use eframe::egui::FontDefinitions;
use eframe::egui::FontFamily;
use eframe::egui::FontId;
use eframe::egui::Frame;
use eframe::egui::Id;
use eframe::egui::Margin;
use eframe::egui::Rgba;
use eframe::egui::Style;
use eframe::egui::TextStyle;
use eframe::egui::Visuals;
use font_loader::system_fonts;
use font_loader::system_fonts::FontPropertyBuilder;
use komorebi_client::KomorebiTheme;
use komorebi_client::SocketMessage;
use komorebi_themes::catppuccin_egui;
use komorebi_themes::Base16Value;
use komorebi_themes::Catppuccin;
use komorebi_themes::CatppuccinValue;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct Komobar {
    pub config: Arc<KomobarConfig>,
    pub render_config: Rc<RefCell<RenderConfig>>,
    pub komorebi_notification_state: Option<Rc<RefCell<KomorebiNotificationState>>>,
    pub left_widgets: Vec<Box<dyn BarWidget>>,
    pub center_widgets: Vec<Box<dyn BarWidget>>,
    pub right_widgets: Vec<Box<dyn BarWidget>>,
    pub rx_gui: Receiver<komorebi_client::Notification>,
    pub rx_config: Receiver<KomobarConfig>,
    pub bg_color: Rc<RefCell<Color32>>,
    pub scale_factor: f32,
}

pub fn apply_theme(
    ctx: &Context,
    theme: KomobarTheme,
    bg_color: Rc<RefCell<Color32>>,
    transparency_alpha: Option<u8>,
) {
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

    // Apply transparency_alpha
    let theme_color = *bg_color.borrow();

    bg_color.replace(theme_color.try_apply_alpha(transparency_alpha));
}

impl Komobar {
    pub fn apply_config(
        &mut self,
        ctx: &Context,
        config: &KomobarConfig,
        previous_notification_state: Option<Rc<RefCell<KomorebiNotificationState>>>,
    ) {
        MAX_LABEL_WIDTH.store(
            config.max_label_width.unwrap_or(400.0) as i32,
            Ordering::SeqCst,
        );

        if let Some(font_family) = &config.font_family {
            tracing::info!("attempting to add custom font family: {font_family}");
            Self::add_custom_font(ctx, font_family);
        }

        let position = config.position.clone().unwrap_or(PositionConfig {
            start: Some(Position {
                x: MONITOR_LEFT.load(Ordering::SeqCst) as f32,
                y: MONITOR_TOP.load(Ordering::SeqCst) as f32,
            }),
            end: Some(Position {
                x: MONITOR_RIGHT.load(Ordering::SeqCst) as f32,
                y: BAR_HEIGHT,
            }),
        });

        if let Some(hwnd) = process_hwnd() {
            let start = position.start.unwrap_or(Position {
                x: MONITOR_LEFT.load(Ordering::SeqCst) as f32,
                y: MONITOR_TOP.load(Ordering::SeqCst) as f32,
            });

            let end = position.end.unwrap_or(Position {
                x: MONITOR_RIGHT.load(Ordering::SeqCst) as f32,
                y: BAR_HEIGHT,
            });

            if end.y == 0.0 {
                tracing::warn!("position.end.y is set to 0.0 which will make your bar invisible on a config reload - this is usually set to 50.0 by default")
            }

            let rect = komorebi_client::Rect {
                left: start.x as i32,
                top: start.y as i32,
                right: end.x as i32,
                bottom: end.y as i32,
            };

            let window = komorebi_client::Window::from(hwnd);
            match window.set_position(&rect, false) {
                Ok(_) => {
                    tracing::info!("updated bar position");
                }
                Err(error) => {
                    tracing::error!("{}", error.to_string())
                }
            }
        }

        match config.theme {
            Some(theme) => {
                apply_theme(ctx, theme, self.bg_color.clone(), config.transparency_alpha);
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
                            apply_theme(
                                ctx,
                                KomobarTheme::from(theme),
                                self.bg_color.clone(),
                                config.transparency_alpha,
                            );

                            let stack_accent = match theme {
                                KomorebiTheme::Catppuccin {
                                    name, stack_border, ..
                                } => stack_border
                                    .unwrap_or(CatppuccinValue::Green)
                                    .color32(name.as_theme()),
                                KomorebiTheme::Base16 {
                                    name, stack_border, ..
                                } => stack_border.unwrap_or(Base16Value::Base0B).color32(name),
                            };

                            if let Some(state) = &self.komorebi_notification_state {
                                state.borrow_mut().stack_accent = Some(stack_accent);
                            }
                        }
                    }
                    Err(_) => {
                        ctx.set_style(Style::default());
                        self.bg_color.replace(Style::default().visuals.panel_fill);
                    }
                }
            }
        }

        // apply rounding to the widgets
        if let Some(
            Grouping::Bar(config) | Grouping::Alignment(config) | Grouping::Widget(config),
        ) = &config.grouping
        {
            if let Some(rounding) = config.rounding {
                ctx.style_mut(|style| {
                    style.visuals.widgets.noninteractive.rounding = rounding.into();
                    style.visuals.widgets.inactive.rounding = rounding.into();
                    style.visuals.widgets.hovered.rounding = rounding.into();
                    style.visuals.widgets.active.rounding = rounding.into();
                    style.visuals.widgets.open.rounding = rounding.into();
                });
            }
        }

        if let Some(font_size) = &config.font_size {
            tracing::info!("attempting to set custom font size: {font_size}");
            Self::set_font_size(ctx, *font_size);
        }

        self.render_config
            .replace(config.new_renderconfig(ctx, *self.bg_color.borrow()));

        let mut komorebi_notification_state = previous_notification_state;
        let mut komorebi_widgets = Vec::new();

        for (idx, widget_config) in config.left_widgets.iter().enumerate() {
            if let WidgetConfig::Komorebi(config) = widget_config {
                komorebi_widgets.push((Komorebi::from(config), idx, Alignment::Left));
            }
        }

        if let Some(center_widgets) = &config.center_widgets {
            for (idx, widget_config) in center_widgets.iter().enumerate() {
                if let WidgetConfig::Komorebi(config) = widget_config {
                    komorebi_widgets.push((Komorebi::from(config), idx, Alignment::Center));
                }
            }
        }

        for (idx, widget_config) in config.right_widgets.iter().enumerate() {
            if let WidgetConfig::Komorebi(config) = widget_config {
                komorebi_widgets.push((Komorebi::from(config), idx, Alignment::Right));
            }
        }

        let mut left_widgets = config
            .left_widgets
            .iter()
            .filter(|config| config.enabled())
            .map(|config| config.as_boxed_bar_widget())
            .collect::<Vec<Box<dyn BarWidget>>>();

        let mut center_widgets = match &config.center_widgets {
            Some(center_widgets) => center_widgets
                .iter()
                .filter(|config| config.enabled())
                .map(|config| config.as_boxed_bar_widget())
                .collect::<Vec<Box<dyn BarWidget>>>(),
            None => vec![],
        };

        let mut right_widgets = config
            .right_widgets
            .iter()
            .filter(|config| config.enabled())
            .map(|config| config.as_boxed_bar_widget())
            .collect::<Vec<Box<dyn BarWidget>>>();

        if !komorebi_widgets.is_empty() {
            komorebi_widgets
                .into_iter()
                .for_each(|(mut widget, idx, side)| {
                    match komorebi_notification_state {
                        None => {
                            komorebi_notification_state =
                                Some(widget.komorebi_notification_state.clone());
                        }
                        Some(ref previous) => {
                            if widget.workspaces.map_or(false, |w| w.enable) {
                                previous.borrow_mut().update_from_config(
                                    &widget.komorebi_notification_state.borrow(),
                                );
                            }

                            widget.komorebi_notification_state = previous.clone();
                        }
                    }

                    let boxed: Box<dyn BarWidget> = Box::new(widget);
                    match side {
                        Alignment::Left => left_widgets[idx] = boxed,
                        Alignment::Center => center_widgets[idx] = boxed,
                        Alignment::Right => right_widgets[idx] = boxed,
                    }
                });
        }

        self.left_widgets = left_widgets;
        self.center_widgets = center_widgets;
        self.right_widgets = right_widgets;

        if let (Some(prev_rect), Some(new_rect)) = (
            &self.config.monitor.work_area_offset,
            &config.monitor.work_area_offset,
        ) {
            if new_rect != prev_rect {
                if let Err(error) = komorebi_client::send_message(
                    &SocketMessage::MonitorWorkAreaOffset(config.monitor.index, *new_rect),
                ) {
                    tracing::error!(
                        "error applying work area offset to monitor '{}': {}",
                        config.monitor.index,
                        error,
                    );
                } else {
                    tracing::info!(
                        "work area offset applied to monitor: {}",
                        config.monitor.index
                    );
                }
            }
        }

        tracing::info!("widget configuration options applied");

        self.komorebi_notification_state = komorebi_notification_state;

        self.config = config.clone().into();
    }

    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rx_gui: Receiver<komorebi_client::Notification>,
        rx_config: Receiver<KomobarConfig>,
        config: Arc<KomobarConfig>,
    ) -> Self {
        let mut komobar = Self {
            config: config.clone(),
            render_config: Rc::new(RefCell::new(RenderConfig::new())),
            komorebi_notification_state: None,
            left_widgets: vec![],
            center_widgets: vec![],
            right_widgets: vec![],
            rx_gui,
            rx_config,
            bg_color: Rc::new(RefCell::new(Style::default().visuals.panel_fill)),
            scale_factor: cc.egui_ctx.native_pixels_per_point().unwrap_or(1.0),
        };

        komobar.apply_config(&cc.egui_ctx, &config, None);
        // needs a double apply the first time for some reason
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
    // Needed for transparency
    fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
        Rgba::TRANSPARENT.to_array()
    }

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
                    self.config.transparency_alpha,
                    self.config.theme,
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

        let mut render_config = self.render_config.borrow_mut();

        let frame = render_config.change_frame_on_bar(frame, &ctx.style());

        CentralPanel::default().frame(frame).show(ctx, |_| {
            // Apply grouping logic for the bar as a whole
            let area_frame = if let Some(frame) = &self.config.frame {
                Frame::none().inner_margin(Margin::symmetric(0.0, frame.inner_margin.y))
            } else {
                Frame::none()
            };

            if !self.left_widgets.is_empty() {
                // Left-aligned widgets layout
                Area::new(Id::new("left_panel"))
                    .anchor(Align2::LEFT_CENTER, [0.0, 0.0]) // Align in the left center of the window
                    .show(ctx, |ui| {
                        let mut left_area_frame = area_frame;
                        if let Some(frame) = &self.config.frame {
                            left_area_frame.inner_margin.left = frame.inner_margin.x;
                        }
                        left_area_frame.show(ui, |ui| {
                            ui.horizontal_centered(|ui| {
                                let mut render_conf = render_config.clone();
                                render_conf.alignment = Some(Alignment::Left);

                                render_config.apply_on_alignment(ui, |ui| {
                                    for w in &mut self.left_widgets {
                                        w.render(ctx, ui, &mut render_conf);
                                    }
                                });
                            });
                        });
                    });
            }

            if !self.right_widgets.is_empty() {
                // Right-aligned widgets layout
                Area::new(Id::new("right_panel"))
                    .anchor(Align2::RIGHT_CENTER, [0.0, 0.0]) // Align in the right center of the window
                    .show(ctx, |ui| {
                        let mut right_area_frame = area_frame;
                        if let Some(frame) = &self.config.frame {
                            right_area_frame.inner_margin.right = frame.inner_margin.x;
                        }
                        right_area_frame.show(ui, |ui| {
                            ui.horizontal_centered(|ui| {
                                let mut render_conf = render_config.clone();
                                render_conf.alignment = Some(Alignment::Right);

                                render_config.apply_on_alignment(ui, |ui| {
                                    for w in &mut self.right_widgets {
                                        w.render(ctx, ui, &mut render_conf);
                                    }
                                });
                            });
                        });
                    });
            }

            if !self.center_widgets.is_empty() {
                // Floating center widgets
                Area::new(Id::new("center_panel"))
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0]) // Align in the center of the window
                    .show(ctx, |ui| {
                        let center_area_frame = area_frame;
                        center_area_frame.show(ui, |ui| {
                            ui.horizontal_centered(|ui| {
                                let mut render_conf = render_config.clone();
                                render_conf.alignment = Some(Alignment::Center);

                                render_config.apply_on_alignment(ui, |ui| {
                                    for w in &mut self.center_widgets {
                                        w.render(ctx, ui, &mut render_conf);
                                    }
                                });
                            });
                        });
                    });
            }
        });
    }
}

#[derive(Copy, Clone)]
pub enum Alignment {
    Left,
    Center,
    Right,
}
