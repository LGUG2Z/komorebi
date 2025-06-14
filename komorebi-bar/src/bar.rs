use crate::config::get_individual_spacing;
use crate::config::KomobarConfig;
use crate::config::KomobarTheme;
use crate::config::MonitorConfigOrIndex;
use crate::config::Position;
use crate::config::PositionConfig;
use crate::process_hwnd;
use crate::render::Color32Ext;
use crate::render::Grouping;
use crate::render::RenderConfig;
use crate::render::RenderExt;
use crate::widgets::komorebi::Komorebi;
use crate::widgets::komorebi::MonitorInfo;
use crate::widgets::widget::BarWidget;
use crate::widgets::widget::WidgetConfig;
use crate::KomorebiEvent;
use crate::AUTO_SELECT_FILL_COLOUR;
use crate::AUTO_SELECT_TEXT_COLOUR;
use crate::BAR_HEIGHT;
use crate::DEFAULT_PADDING;
use crate::MAX_LABEL_WIDTH;
use crate::MONITOR_LEFT;
use crate::MONITOR_RIGHT;
use crate::MONITOR_TOP;
use crossbeam_channel::Receiver;
use crossbeam_channel::TryRecvError;
use eframe::egui::Align;
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
use eframe::egui::Layout;
use eframe::egui::Margin;
use eframe::egui::PointerButton;
use eframe::egui::Rgba;
use eframe::egui::Style;
use eframe::egui::TextStyle;
use eframe::egui::Vec2;
use eframe::egui::Visuals;
use font_loader::system_fonts;
use font_loader::system_fonts::FontPropertyBuilder;
use komorebi_client::Colour;
use komorebi_client::MonitorNotification;
use komorebi_client::NotificationEvent;
use komorebi_client::PathExt;
use komorebi_client::SocketMessage;
use komorebi_client::VirtualDesktopNotification;
use komorebi_themes::catppuccin_egui;
use komorebi_themes::Base16Wrapper;
use komorebi_themes::Catppuccin;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::ChildStdin;
use std::process::Command;
use std::process::Stdio;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

lazy_static! {
    static ref SESSION_STDIN: Mutex<Option<ChildStdin>> = Mutex::new(None);
}

fn start_powershell() -> Result<()> {
    // found running session, do nothing
    if SESSION_STDIN.lock().as_mut().is_some() {
        tracing::debug!("PowerShell session already started");
        return Ok(());
    }

    tracing::debug!("Starting PowerShell session");

    let mut child = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-Command", "-"])
        .stdin(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;

    let stdin = child.stdin.take().expect("stdin piped");

    // Store stdin for later commands
    let mut session_stdin = SESSION_STDIN.lock();
    *session_stdin = Option::from(stdin);

    Ok(())
}

fn stop_powershell() -> Result<()> {
    tracing::debug!("Stopping PowerShell session");

    if let Some(mut session_stdin) = SESSION_STDIN.lock().take() {
        if let Err(e) = session_stdin.write_all(b"exit\n") {
            tracing::error!(error = %e, "failed to write exit command to PowerShell stdin");
            return Err(e);
        }
        if let Err(e) = session_stdin.flush() {
            tracing::error!(error = %e, "failed to flush PowerShell stdin");
            return Err(e);
        }

        tracing::debug!("PowerShell session stopped");
    } else {
        tracing::debug!("PowerShell session already stopped");
    }

    Ok(())
}

pub fn exec_powershell(cmd: &str) -> Result<()> {
    if let Some(session_stdin) = SESSION_STDIN.lock().as_mut() {
        if let Err(e) = writeln!(session_stdin, "{}", cmd) {
            tracing::error!(error = %e, cmd = cmd, "failed to write command to PowerShell stdin");
            return Err(e);
        }

        if let Err(e) = session_stdin.flush() {
            tracing::error!(error = %e, "failed to flush PowerShell stdin");
            return Err(e);
        }

        return Ok(());
    }

    Err(Error::new(
        ErrorKind::NotFound,
        "PowerShell session not started",
    ))
}

pub struct Komobar {
    pub hwnd: Option<isize>,
    pub monitor_index: Option<usize>,
    pub disabled: bool,
    pub config: KomobarConfig,
    pub render_config: Rc<RefCell<RenderConfig>>,
    pub monitor_info: Option<Rc<RefCell<MonitorInfo>>>,
    pub left_widgets: Vec<Box<dyn BarWidget>>,
    pub center_widgets: Vec<Box<dyn BarWidget>>,
    pub right_widgets: Vec<Box<dyn BarWidget>>,
    pub rx_gui: Receiver<KomorebiEvent>,
    pub rx_config: Receiver<KomobarConfig>,
    pub bg_color: Rc<RefCell<Color32>>,
    pub bg_color_with_alpha: Rc<RefCell<Color32>>,
    pub scale_factor: f32,
    pub size_rect: komorebi_client::Rect,
    pub work_area_offset: komorebi_client::Rect,
    applied_theme_on_first_frame: bool,
    mouse_follows_focus: bool,
    input_config: InputConfig,
}

struct InputConfig {
    accumulated_scroll_delta: Vec2,
    act_on_vertical_scroll: bool,
    act_on_horizontal_scroll: bool,
    vertical_scroll_threshold: f32,
    horizontal_scroll_threshold: f32,
    vertical_scroll_max_threshold: f32,
    horizontal_scroll_max_threshold: f32,
}

pub fn apply_theme(
    ctx: &Context,
    theme: KomobarTheme,
    bg_color: Rc<RefCell<Color32>>,
    bg_color_with_alpha: Rc<RefCell<Color32>>,
    transparency_alpha: Option<u8>,
    grouping: Option<Grouping>,
    render_config: Rc<RefCell<RenderConfig>>,
) {
    let (auto_select_fill, auto_select_text) = match theme {
        KomobarTheme::Catppuccin {
            name: catppuccin,
            accent: catppuccin_value,
            auto_select_fill: catppuccin_auto_select_fill,
            auto_select_text: catppuccin_auto_select_text,
        } => {
            match catppuccin {
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
            }

            (
                catppuccin_auto_select_fill.map(|c| c.color32(catppuccin.as_theme())),
                catppuccin_auto_select_text.map(|c| c.color32(catppuccin.as_theme())),
            )
        }
        KomobarTheme::Base16 {
            name: base16,
            accent: base16_value,
            auto_select_fill: base16_auto_select_fill,
            auto_select_text: base16_auto_select_text,
        } => {
            ctx.set_style(base16.style());
            let base16_value = base16_value.unwrap_or_default();
            let accent = base16_value.color32(Base16Wrapper::Base16(base16));

            ctx.style_mut(|style| {
                style.visuals.selection.stroke.color = accent;
                style.visuals.widgets.hovered.fg_stroke.color = accent;
                style.visuals.widgets.active.fg_stroke.color = accent;
            });

            bg_color.replace(base16.background());

            (
                base16_auto_select_fill.map(|c| c.color32(Base16Wrapper::Base16(base16))),
                base16_auto_select_text.map(|c| c.color32(Base16Wrapper::Base16(base16))),
            )
        }
        KomobarTheme::Custom {
            colours,
            accent: base16_value,
            auto_select_fill: base16_auto_select_fill,
            auto_select_text: base16_auto_select_text,
        } => {
            let background = colours.background();
            ctx.set_style(colours.style());
            let base16_value = base16_value.unwrap_or_default();
            let accent = base16_value.color32(Base16Wrapper::Custom(colours.clone()));

            ctx.style_mut(|style| {
                style.visuals.selection.stroke.color = accent;
                style.visuals.widgets.hovered.fg_stroke.color = accent;
                style.visuals.widgets.active.fg_stroke.color = accent;
            });

            bg_color.replace(background);

            (
                base16_auto_select_fill.map(|c| c.color32(Base16Wrapper::Custom(colours.clone()))),
                base16_auto_select_text.map(|c| c.color32(Base16Wrapper::Custom(colours.clone()))),
            )
        }
    };

    AUTO_SELECT_FILL_COLOUR.store(
        auto_select_fill.map_or(0, |c| Colour::from(c).into()),
        Ordering::SeqCst,
    );
    AUTO_SELECT_TEXT_COLOUR.store(
        auto_select_text.map_or(0, |c| Colour::from(c).into()),
        Ordering::SeqCst,
    );

    // Apply transparency_alpha
    let theme_color = *bg_color.borrow();

    bg_color_with_alpha.replace(theme_color.try_apply_alpha(transparency_alpha));

    // apply rounding to the widgets
    if let Some(Grouping::Bar(config) | Grouping::Alignment(config) | Grouping::Widget(config)) =
        &grouping
    {
        if let Some(rounding) = config.rounding {
            ctx.style_mut(|style| {
                style.visuals.widgets.noninteractive.corner_radius = rounding.into();
                style.visuals.widgets.inactive.corner_radius = rounding.into();
                style.visuals.widgets.hovered.corner_radius = rounding.into();
                style.visuals.widgets.active.corner_radius = rounding.into();
                style.visuals.widgets.open.corner_radius = rounding.into();
            });
        }
    }

    // Update RenderConfig's background_color so that widgets will have the new color
    render_config.borrow_mut().background_color = *bg_color.borrow();
}

impl Komobar {
    pub fn apply_config(
        &mut self,
        ctx: &Context,
        previous_monitor_info: Option<Rc<RefCell<MonitorInfo>>>,
    ) {
        MAX_LABEL_WIDTH.store(
            self.config.max_label_width.unwrap_or(400.0) as i32,
            Ordering::SeqCst,
        );

        if let Some(font_family) = &self.config.font_family {
            tracing::info!("attempting to add custom font family: {font_family}");
            Self::add_custom_font(ctx, font_family);
        }

        // Update the `size_rect` so that the bar position can be changed on the EGUI update
        // function
        self.update_size_rect();

        self.try_apply_theme(ctx);

        if let Some(font_size) = &self.config.font_size {
            tracing::info!("attempting to set custom font size: {font_size}");
            Self::set_font_size(ctx, *font_size);
        }

        self.render_config.replace((&self.config).new_renderconfig(
            ctx,
            *self.bg_color.borrow(),
            self.config.icon_scale,
        ));

        let mut monitor_info = previous_monitor_info;
        let mut komorebi_widgets = Vec::new();

        for (idx, widget_config) in self.config.left_widgets.iter().enumerate() {
            if let WidgetConfig::Komorebi(config) = widget_config {
                komorebi_widgets.push((Komorebi::from(config), idx, Alignment::Left));
            }
        }

        if let Some(center_widgets) = &self.config.center_widgets {
            for (idx, widget_config) in center_widgets.iter().enumerate() {
                if let WidgetConfig::Komorebi(config) = widget_config {
                    komorebi_widgets.push((Komorebi::from(config), idx, Alignment::Center));
                }
            }
        }

        for (idx, widget_config) in self.config.right_widgets.iter().enumerate() {
            if let WidgetConfig::Komorebi(config) = widget_config {
                komorebi_widgets.push((Komorebi::from(config), idx, Alignment::Right));
            }
        }

        let mut left_widgets = self
            .config
            .left_widgets
            .iter()
            .filter(|config| config.enabled())
            .map(|config| config.as_boxed_bar_widget())
            .collect::<Vec<Box<dyn BarWidget>>>();

        let mut center_widgets = match &self.config.center_widgets {
            Some(center_widgets) => center_widgets
                .iter()
                .filter(|config| config.enabled())
                .map(|config| config.as_boxed_bar_widget())
                .collect::<Vec<Box<dyn BarWidget>>>(),
            None => vec![],
        };

        let mut right_widgets = self
            .config
            .right_widgets
            .iter()
            .filter(|config| config.enabled())
            .map(|config| config.as_boxed_bar_widget())
            .collect::<Vec<Box<dyn BarWidget>>>();

        if !komorebi_widgets.is_empty() {
            komorebi_widgets
                .into_iter()
                .for_each(|(mut widget, idx, side)| {
                    match monitor_info {
                        None => {
                            monitor_info = Some(widget.monitor_info.clone());
                        }
                        Some(ref previous) => {
                            if widget.workspaces_old.is_some_and(|w| w.enable) {
                                previous
                                    .borrow_mut()
                                    .update_from_self(&widget.monitor_info.borrow());
                            }

                            widget.monitor_info = previous.clone();
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

        right_widgets.reverse();

        self.left_widgets = left_widgets;
        self.center_widgets = center_widgets;
        self.right_widgets = right_widgets;

        let (usr_monitor_index, config_work_area_offset) = match &self.config.monitor {
            MonitorConfigOrIndex::MonitorConfig(monitor_config) => {
                (monitor_config.index, monitor_config.work_area_offset)
            }
            MonitorConfigOrIndex::Index(idx) => (*idx, None),
        };

        let mapped_info = self.monitor_info.as_ref().map(|info| {
            let monitor = info.borrow();
            (
                monitor.monitor_usr_idx_map.get(&usr_monitor_index).copied(),
                monitor.mouse_follows_focus,
            )
        });

        if let Some(info) = mapped_info {
            self.monitor_index = info.0;
            self.mouse_follows_focus = info.1;
        }

        if let Some(monitor_index) = self.monitor_index {
            if let (prev_rect, Some(new_rect)) = (&self.work_area_offset, &config_work_area_offset)
            {
                if new_rect != prev_rect {
                    self.work_area_offset = *new_rect;
                    if let Err(error) = komorebi_client::send_message(
                        &SocketMessage::MonitorWorkAreaOffset(monitor_index, *new_rect),
                    ) {
                        tracing::error!(
                            "error applying work area offset to monitor '{}': {}",
                            monitor_index,
                            error,
                        );
                    } else {
                        tracing::info!("work area offset applied to monitor: {}", monitor_index);
                    }
                }
            } else if let Some(height) = self.config.height.or(Some(BAR_HEIGHT)) {
                // We only add the `bottom_margin` to the work_area_offset since the top margin is
                // already considered on the `size_rect.top`
                let bottom_margin = self
                    .config
                    .margin
                    .as_ref()
                    .map_or(0, |v| v.to_individual(0.0).bottom as i32);
                let new_rect = komorebi_client::Rect {
                    left: 0,
                    top: (height as i32)
                        + (self.size_rect.top - MONITOR_TOP.load(Ordering::SeqCst))
                        + bottom_margin,
                    right: 0,
                    bottom: (height as i32)
                        + (self.size_rect.top - MONITOR_TOP.load(Ordering::SeqCst))
                        + bottom_margin,
                };

                if new_rect != self.work_area_offset {
                    self.work_area_offset = new_rect;
                    if let Err(error) = komorebi_client::send_message(
                        &SocketMessage::MonitorWorkAreaOffset(monitor_index, new_rect),
                    ) {
                        tracing::error!(
                            "error applying work area offset to monitor '{monitor_index}': {error}"
                        );
                    } else {
                        tracing::info!("work area offset applied to monitor: {monitor_index}",);
                    }
                }
            }
        } else if self.monitor_info.is_some() && !self.disabled {
            tracing::warn!("couldn't find the monitor index of this bar! Disabling the bar until the monitor connects...");
            self.disabled = true;
        } else {
            tracing::warn!("couldn't find the monitor index of this bar, if the bar is starting up this is normal until it receives the first state from komorebi.");
            self.disabled = true;
        }

        if let Some(mouse) = &self.config.mouse {
            self.input_config.act_on_vertical_scroll =
                mouse.on_scroll_up.is_some() || mouse.on_scroll_down.is_some();
            self.input_config.act_on_horizontal_scroll =
                mouse.on_scroll_left.is_some() || mouse.on_scroll_right.is_some();
            self.input_config.vertical_scroll_threshold = mouse
                .vertical_scroll_threshold
                .unwrap_or(30.0)
                .clamp(10.0, 300.0);
            self.input_config.horizontal_scroll_threshold = mouse
                .horizontal_scroll_threshold
                .unwrap_or(30.0)
                .clamp(10.0, 300.0);
            // limit how many "ticks" can be accumulated
            self.input_config.vertical_scroll_max_threshold =
                self.input_config.vertical_scroll_threshold * 3.0;
            self.input_config.horizontal_scroll_max_threshold =
                self.input_config.horizontal_scroll_threshold * 3.0;

            if mouse.has_command() {
                start_powershell().unwrap_or_else(|_| {
                    tracing::error!("failed to start powershell session");
                });
            } else {
                stop_powershell().unwrap_or_else(|_| {
                    tracing::error!("failed to stop powershell session");
                });
            }
        }

        tracing::info!("widget configuration options applied");

        self.monitor_info = monitor_info;
    }

    /// Updates the `size_rect` field. Returns a bool indicating if the field was changed or not
    fn update_size_rect(&mut self) {
        let position = self.config.position.clone().unwrap_or(PositionConfig {
            start: Some(Position {
                x: MONITOR_LEFT.load(Ordering::SeqCst) as f32,
                y: MONITOR_TOP.load(Ordering::SeqCst) as f32,
            }),
            end: Some(Position {
                x: MONITOR_RIGHT.load(Ordering::SeqCst) as f32,
                y: BAR_HEIGHT,
            }),
        });

        let mut start = position.start.unwrap_or(Position {
            x: MONITOR_LEFT.load(Ordering::SeqCst) as f32,
            y: MONITOR_TOP.load(Ordering::SeqCst) as f32,
        });

        let mut end = position.end.unwrap_or(Position {
            x: MONITOR_RIGHT.load(Ordering::SeqCst) as f32,
            y: BAR_HEIGHT,
        });

        if let Some(height) = self.config.height {
            end.y = height;
        }

        let margin = get_individual_spacing(0.0, &self.config.margin);

        start.y += margin.top;
        start.x += margin.left;
        end.x -= margin.left + margin.right;

        if end.y == 0.0 {
            tracing::warn!("position.end.y is set to 0.0 which will make your bar invisible on a config reload - this is usually set to 50.0 by default")
        }

        self.size_rect = komorebi_client::Rect {
            left: start.x as i32,
            top: start.y as i32,
            right: end.x as i32,
            bottom: end.y as i32,
        };
    }

    fn try_apply_theme(&mut self, ctx: &Context) {
        match &self.config.theme {
            Some(theme) => {
                apply_theme(
                    ctx,
                    theme.clone(),
                    self.bg_color.clone(),
                    self.bg_color_with_alpha.clone(),
                    self.config.transparency_alpha,
                    self.config.grouping,
                    self.render_config.clone(),
                );
            }
            None => {
                let home_dir: PathBuf = std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(
                    |_| dirs::home_dir().expect("there is no home directory"),
                    |home_path| {
                        let home = home_path.replace_env();

                        assert!(
                            home.is_dir(),
                            "$Env:KOMOREBI_CONFIG_HOME is set to '{}', which is not a valid directory",
                            home_path
                        );

                        home

                    },
                );

                let bar_transparency_alpha = self.config.transparency_alpha;
                let bar_grouping = self.config.grouping;
                let config = home_dir.join("komorebi.json");
                match komorebi_client::StaticConfig::read(&config) {
                    Ok(config) => {
                        if let Some(theme) = config.theme {
                            apply_theme(
                                ctx,
                                KomobarTheme::from(theme),
                                self.bg_color.clone(),
                                self.bg_color_with_alpha.clone(),
                                bar_transparency_alpha,
                                bar_grouping,
                                self.render_config.clone(),
                            );
                        }
                    }
                    Err(_) => {
                        ctx.set_style(Style::default());
                        self.bg_color.replace(Style::default().visuals.panel_fill);

                        // apply rounding to the widgets since we didn't call `apply_theme`
                        if let Some(
                            Grouping::Bar(config)
                            | Grouping::Alignment(config)
                            | Grouping::Widget(config),
                        ) = &bar_grouping
                        {
                            if let Some(rounding) = config.rounding {
                                ctx.style_mut(|style| {
                                    style.visuals.widgets.noninteractive.corner_radius =
                                        rounding.into();
                                    style.visuals.widgets.inactive.corner_radius = rounding.into();
                                    style.visuals.widgets.hovered.corner_radius = rounding.into();
                                    style.visuals.widgets.active.corner_radius = rounding.into();
                                    style.visuals.widgets.open.corner_radius = rounding.into();
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rx_gui: Receiver<KomorebiEvent>,
        rx_config: Receiver<KomobarConfig>,
        config: KomobarConfig,
    ) -> Self {
        let mut komobar = Self {
            hwnd: process_hwnd(),
            monitor_index: None,
            disabled: false,
            config,
            render_config: Rc::new(RefCell::new(RenderConfig::new())),
            monitor_info: None,
            left_widgets: vec![],
            center_widgets: vec![],
            right_widgets: vec![],
            rx_gui,
            rx_config,
            bg_color: Rc::new(RefCell::new(Style::default().visuals.panel_fill)),
            bg_color_with_alpha: Rc::new(RefCell::new(Style::default().visuals.panel_fill)),
            scale_factor: cc.egui_ctx.native_pixels_per_point().unwrap_or(1.0),
            size_rect: komorebi_client::Rect::default(),
            work_area_offset: komorebi_client::Rect::default(),
            applied_theme_on_first_frame: false,
            mouse_follows_focus: false,
            input_config: InputConfig {
                accumulated_scroll_delta: Vec2::new(0.0, 0.0),
                act_on_vertical_scroll: false,
                act_on_horizontal_scroll: false,
                vertical_scroll_threshold: 0.0,
                horizontal_scroll_threshold: 0.0,
                vertical_scroll_max_threshold: 0.0,
                horizontal_scroll_max_threshold: 0.0,
            },
        };

        komobar.apply_config(&cc.egui_ctx, None);
        // needs a double apply the first time for some reason
        komobar.apply_config(&cc.egui_ctx, None);

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

        let mut fallbacks = HashMap::new();

        fallbacks.insert("Microsoft YaHei", "C:\\Windows\\Fonts\\msyh.ttc"); // chinese
        fallbacks.insert("Malgun Gothic", "C:\\Windows\\Fonts\\malgun.ttf"); // korean
        fallbacks.insert("Leelawadee UI", "C:\\Windows\\Fonts\\LeelawUI.ttf"); // thai

        for (name, path) in fallbacks {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert(name.to_owned(), Arc::from(FontData::from_owned(bytes)));

                for family in [FontFamily::Proportional, FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family)
                        .or_default()
                        .insert(0, name.to_owned());
                }
            }
        }

        let property = FontPropertyBuilder::new().family(name).build();

        if let Some((font, _)) = system_fonts::get(&property) {
            fonts
                .font_data
                .insert(name.to_owned(), Arc::new(FontData::from_owned(font)));

            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, name.to_owned());
            }
        }

        // Tell egui to use these fonts:
        ctx.set_fonts(fonts);
    }

    pub fn position_bar(&self) {
        if let Some(hwnd) = self.hwnd {
            let window = komorebi_client::Window::from(hwnd);
            match window.set_position(&self.size_rect, false) {
                Ok(_) => {
                    tracing::info!("updated bar position");
                }
                Err(error) => {
                    tracing::error!("{error}")
                }
            }
        }
    }

    fn update_monitor_coordinates(&mut self, monitor_size: &komorebi_client::Rect) {
        // Store the new monitor coordinates
        MONITOR_TOP.store(monitor_size.top, Ordering::SeqCst);
        MONITOR_LEFT.store(monitor_size.left, Ordering::SeqCst);
        MONITOR_RIGHT.store(monitor_size.right, Ordering::SeqCst);

        // Since the `config.position` is changed on `main.rs` we need to update it here.
        // If the user had set up some `start` position, that will be overriden here
        // since we have no way to know what was that value since it might have been
        // changed on `main.rs`. However if the users use the new configs this won't be
        // a problem for them.
        if let Some(start) = self.config.position.as_mut().and_then(|p| p.start.as_mut()) {
            start.x = monitor_size.left as f32;
            start.y = monitor_size.top as f32;
        }
    }
}
impl eframe::App for Komobar {
    // Needed for transparency
    fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
        Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.hwnd.is_none() {
            self.hwnd = process_hwnd();
        }

        if self.scale_factor != ctx.native_pixels_per_point().unwrap_or(1.0) {
            self.scale_factor = ctx.native_pixels_per_point().unwrap_or(1.0);
            self.apply_config(ctx, self.monitor_info.clone());
        }

        if let Ok(updated_config) = self.rx_config.try_recv() {
            self.config = updated_config;
            self.apply_config(ctx, self.monitor_info.clone());
        }

        match self.rx_gui.try_recv() {
            Err(error) => match error {
                TryRecvError::Empty => {}
                TryRecvError::Disconnected => {
                    tracing::error!(
                        "failed to receive komorebi notification on gui thread: {error}"
                    );
                }
            },
            Ok(KomorebiEvent::Notification(notification)) => {
                let state = &notification.state;
                let usr_monitor_index = match &self.config.monitor {
                    MonitorConfigOrIndex::MonitorConfig(monitor_config) => monitor_config.index,
                    MonitorConfigOrIndex::Index(idx) => *idx,
                };
                let monitor_index = state.monitor_usr_idx_map.get(&usr_monitor_index).copied();
                self.monitor_index = monitor_index;
                let mut should_apply_config = false;

                match notification.event {
                    NotificationEvent::VirtualDesktop(
                        VirtualDesktopNotification::EnteredAssociatedVirtualDesktop,
                    ) => {
                        tracing::debug!(
                            "back on komorebi's associated virtual desktop - restoring bar"
                        );
                        if let Some(hwnd) = self.hwnd {
                            komorebi_client::WindowsApi::restore_window(hwnd);
                        }
                    }
                    NotificationEvent::VirtualDesktop(
                        VirtualDesktopNotification::LeftAssociatedVirtualDesktop,
                    ) => {
                        tracing::debug!(
                            "no longer on komorebi's associated virtual desktop - minimizing bar"
                        );
                        if let Some(hwnd) = self.hwnd {
                            komorebi_client::WindowsApi::minimize_window(hwnd);
                        }
                    }
                    _ => {}
                }

                if self.monitor_index.is_none()
                    || self
                        .monitor_index
                        .is_some_and(|idx| idx >= state.monitors.elements().len())
                {
                    if !self.disabled {
                        // Monitor for this bar got disconnected lets disable the bar until it
                        // reconnects
                        self.disabled = true;
                        tracing::warn!(
                            "This bar's monitor got disconnected. The bar will be disabled until it reconnects..."
                        );
                    }
                    return;
                } else {
                    if self.disabled {
                        tracing::info!("Found this bar's monitor. The bar will be enabled!");

                        // Restore the bar in case it has been minimized when the monitor
                        // disconnected
                        if let Some(hwnd) = self.hwnd {
                            let window = komorebi_client::Window::from(hwnd);
                            if window.is_miminized() {
                                komorebi_client::WindowsApi::restore_window(hwnd);
                            }
                        }

                        // Reset the current `work_area_offset` so that it gets recalculated and
                        // properly applied again, since if the monitor has connected for the first
                        // time it won't have the work_area_offset applied but the bar thinks it
                        // does.
                        self.work_area_offset = komorebi_client::Rect::default();

                        should_apply_config = true;
                    }
                    self.disabled = false;
                }

                if matches!(
                    notification.event,
                    NotificationEvent::Monitor(MonitorNotification::DisplayConnectionChange)
                ) {
                    let monitor_index = self.monitor_index.expect("should have a monitor index");

                    let monitor_size = state.monitors.elements()[monitor_index].size();

                    self.update_monitor_coordinates(monitor_size);

                    should_apply_config = true;
                }

                if self.disabled {
                    return;
                }

                // Check if monitor coordinates/size has changed
                if let Some(monitor_index) = self.monitor_index {
                    let monitor_size = state.monitors.elements()[monitor_index].size();
                    let top = MONITOR_TOP.load(Ordering::SeqCst);
                    let left = MONITOR_LEFT.load(Ordering::SeqCst);
                    let right = MONITOR_RIGHT.load(Ordering::SeqCst);
                    let rect = komorebi_client::Rect {
                        top,
                        left,
                        bottom: monitor_size.bottom,
                        right,
                    };
                    if *monitor_size != rect {
                        tracing::info!(
                            "Monitor coordinates/size has changed, storing new coordinates: {:#?}",
                            monitor_size
                        );

                        self.update_monitor_coordinates(monitor_size);

                        should_apply_config = true;
                    }
                }

                if let Some(monitor_info) = &self.monitor_info {
                    monitor_info.borrow_mut().update(
                        self.monitor_index,
                        notification.state,
                        self.render_config.borrow().show_all_icons,
                    );
                    handle_notification(
                        ctx,
                        notification.event,
                        self.bg_color.clone(),
                        self.bg_color_with_alpha.clone(),
                        self.config.transparency_alpha,
                        self.config.grouping,
                        self.config.theme.clone(),
                        self.render_config.clone(),
                    );
                }

                if should_apply_config {
                    self.apply_config(ctx, self.monitor_info.clone());

                    // Reposition the Bar
                    self.position_bar();
                }
            }
            Ok(KomorebiEvent::Reconnect) => {
                if let Some(monitor_index) = self.monitor_index {
                    if let Err(error) = komorebi_client::send_message(
                        &SocketMessage::MonitorWorkAreaOffset(monitor_index, self.work_area_offset),
                    ) {
                        tracing::error!(
                            "error applying work area offset to monitor '{}': {}",
                            monitor_index,
                            error,
                        );
                    } else {
                        tracing::info!("work area offset applied to monitor: {}", monitor_index);
                    }
                }
            }
        }

        if self.disabled {
            // The check for disabled is performed above, if we get here and the bar is still
            // disabled then we should return without drawing anything.
            return;
        }

        if !self.applied_theme_on_first_frame {
            self.try_apply_theme(ctx);
            self.applied_theme_on_first_frame = true;
        }

        // Check if egui's Window size is the expected one, if not, update it
        if let Some(current_rect) = ctx.input(|i| i.viewport().outer_rect) {
            // Get the correct size according to scale factor
            let current_rect = komorebi_client::Rect {
                left: (current_rect.min.x * self.scale_factor) as i32,
                top: (current_rect.min.y * self.scale_factor) as i32,
                right: ((current_rect.max.x - current_rect.min.x) * self.scale_factor) as i32,
                bottom: ((current_rect.max.y - current_rect.min.y) * self.scale_factor) as i32,
            };

            if self.size_rect != current_rect {
                self.position_bar();
            }
        }

        let frame = match &self.config.padding {
            None => {
                if let Some(frame) = &self.config.frame {
                    Frame::NONE
                        .inner_margin(Margin::symmetric(
                            frame.inner_margin.x as i8,
                            frame.inner_margin.y as i8,
                        ))
                        .fill(*self.bg_color_with_alpha.borrow())
                } else {
                    Frame::NONE
                        .inner_margin(Margin::same(0))
                        .fill(*self.bg_color_with_alpha.borrow())
                }
            }
            Some(padding) => {
                let padding = padding.to_individual(DEFAULT_PADDING);
                Frame::NONE
                    .inner_margin(Margin {
                        top: padding.top as i8,
                        bottom: padding.bottom as i8,
                        left: padding.left as i8,
                        right: padding.right as i8,
                    })
                    .fill(*self.bg_color_with_alpha.borrow())
            }
        };

        let mut render_config = self.render_config.borrow_mut();

        let frame = render_config.change_frame_on_bar(frame, &ctx.style());

        CentralPanel::default().frame(frame).show(ctx, |ui| {
            if let Some(mouse_config) = &self.config.mouse {
                let command = if ui
                    .input(|i| i.pointer.button_double_clicked(PointerButton::Primary))
                {
                    tracing::debug!("Input: primary button double clicked");
                    &mouse_config.on_primary_double_click
                } else if ui.input(|i| i.pointer.button_clicked(PointerButton::Secondary)) {
                    tracing::debug!("Input: secondary button clicked");
                    &mouse_config.on_secondary_click
                } else if ui.input(|i| i.pointer.button_clicked(PointerButton::Middle)) {
                    tracing::debug!("Input: middle button clicked");
                    &mouse_config.on_middle_click
                } else if ui.input(|i| i.pointer.button_clicked(PointerButton::Extra1)) {
                    tracing::debug!("Input: extra1 button clicked");
                    &mouse_config.on_extra1_click
                } else if ui.input(|i| i.pointer.button_clicked(PointerButton::Extra2)) {
                    tracing::debug!("Input: extra2 button clicked");
                    &mouse_config.on_extra2_click
                } else if self.input_config.act_on_vertical_scroll
                    || self.input_config.act_on_horizontal_scroll
                {
                    let scroll_delta = ui.input(|input| input.smooth_scroll_delta);

                    self.input_config.accumulated_scroll_delta += scroll_delta;

                    if scroll_delta.y != 0.0 && self.input_config.act_on_vertical_scroll {
                        // Do not store more than the max threshold
                        self.input_config.accumulated_scroll_delta.y =
                            self.input_config.accumulated_scroll_delta.y.clamp(
                                -self.input_config.vertical_scroll_max_threshold,
                                self.input_config.vertical_scroll_max_threshold,
                            );

                        // When the accumulated scroll passes the threshold, trigger a tick.
                        if self.input_config.accumulated_scroll_delta.y.abs()
                            >= self.input_config.vertical_scroll_threshold
                        {
                            let direction_command =
                                if self.input_config.accumulated_scroll_delta.y > 0.0 {
                                    &mouse_config.on_scroll_up
                                } else {
                                    &mouse_config.on_scroll_down
                                };

                            // Remove one tick's worth of scroll from the accumulator, preserving any excess.
                            self.input_config.accumulated_scroll_delta.y -=
                                self.input_config.vertical_scroll_threshold
                                    * self.input_config.accumulated_scroll_delta.y.signum();

                            tracing::debug!(
                                "Input: vertical scroll ticked. excess: {} | threshold: {}",
                                self.input_config.accumulated_scroll_delta.y,
                                self.input_config.vertical_scroll_threshold
                            );

                            direction_command
                        } else {
                            &None
                        }
                    } else if scroll_delta.x != 0.0 && self.input_config.act_on_horizontal_scroll {
                        // Do not store more than the max threshold
                        self.input_config.accumulated_scroll_delta.x =
                            self.input_config.accumulated_scroll_delta.x.clamp(
                                -self.input_config.horizontal_scroll_max_threshold,
                                self.input_config.horizontal_scroll_max_threshold,
                            );

                        // When the accumulated scroll passes the threshold, trigger a tick.
                        if self.input_config.accumulated_scroll_delta.x.abs()
                            >= self.input_config.horizontal_scroll_threshold
                        {
                            let direction_command =
                                if self.input_config.accumulated_scroll_delta.x > 0.0 {
                                    &mouse_config.on_scroll_left
                                } else {
                                    &mouse_config.on_scroll_right
                                };

                            // Remove one tick's worth of scroll from the accumulator, preserving any excess.
                            self.input_config.accumulated_scroll_delta.x -=
                                self.input_config.horizontal_scroll_threshold
                                    * self.input_config.accumulated_scroll_delta.x.signum();

                            tracing::debug!(
                                "Input: horizontal scroll ticked. excess: {} | threshold: {}",
                                self.input_config.accumulated_scroll_delta.x,
                                self.input_config.horizontal_scroll_threshold
                            );

                            direction_command
                        } else {
                            &None
                        }
                    } else {
                        &None
                    }
                } else {
                    &None
                };

                if let Some(command) = command {
                    command.execute(self.mouse_follows_focus);
                }
            }

            // Apply grouping logic for the bar as a whole
            let area_frame = if let Some(frame) = &self.config.frame {
                Frame::NONE
                    .inner_margin(Margin::symmetric(0, frame.inner_margin.y as i8))
                    .outer_margin(Margin::same(0))
            } else {
                Frame::NONE
                    .inner_margin(Margin::same(0))
                    .outer_margin(Margin::same(0))
            };

            let available_height = ui.max_rect().max.y;
            ctx.style_mut(|style| {
                style.spacing.interact_size.y = available_height;
            });

            if !self.left_widgets.is_empty() {
                // Left-aligned widgets layout
                Area::new(Id::new("left_panel"))
                    .anchor(Align2::LEFT_CENTER, [0.0, 0.0]) // Align in the left center of the window
                    .show(ctx, |ui| {
                        let mut left_area_frame = area_frame;
                        if let Some(padding) = self
                            .config
                            .padding
                            .as_ref()
                            .map(|s| s.to_individual(DEFAULT_PADDING))
                        {
                            left_area_frame.inner_margin.left = padding.left as i8;
                            left_area_frame.inner_margin.top = padding.top as i8;
                            left_area_frame.inner_margin.bottom = padding.bottom as i8;
                        } else if let Some(frame) = &self.config.frame {
                            left_area_frame.inner_margin.left = frame.inner_margin.x as i8;
                            left_area_frame.inner_margin.top = frame.inner_margin.y as i8;
                            left_area_frame.inner_margin.bottom = frame.inner_margin.y as i8;
                        }

                        left_area_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
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
                        if let Some(padding) = self
                            .config
                            .padding
                            .as_ref()
                            .map(|s| s.to_individual(DEFAULT_PADDING))
                        {
                            right_area_frame.inner_margin.right = padding.right as i8;
                            right_area_frame.inner_margin.top = padding.top as i8;
                            right_area_frame.inner_margin.bottom = padding.bottom as i8;
                        } else if let Some(frame) = &self.config.frame {
                            right_area_frame.inner_margin.right = frame.inner_margin.x as i8;
                            right_area_frame.inner_margin.top = frame.inner_margin.y as i8;
                            right_area_frame.inner_margin.bottom = frame.inner_margin.y as i8;
                        }

                        right_area_frame.show(ui, |ui| {
                            let initial_size = Vec2 {
                                x: ui.available_size_before_wrap().x,
                                y: ui.spacing().interact_size.y,
                            };
                            ui.allocate_ui_with_layout(
                                initial_size,
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    let mut render_conf = render_config.clone();
                                    render_conf.alignment = Some(Alignment::Right);

                                    render_config.apply_on_alignment(ui, |ui| {
                                        for w in &mut self.right_widgets {
                                            w.render(ctx, ui, &mut render_conf);
                                        }
                                    });
                                },
                            );
                        });
                    });
            }

            if !self.center_widgets.is_empty() {
                // Floating center widgets
                Area::new(Id::new("center_panel"))
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0]) // Align in the center of the window
                    .show(ctx, |ui| {
                        let mut center_area_frame = area_frame;
                        if let Some(padding) = self
                            .config
                            .padding
                            .as_ref()
                            .map(|s| s.to_individual(DEFAULT_PADDING))
                        {
                            center_area_frame.inner_margin.top = padding.top as i8;
                            center_area_frame.inner_margin.bottom = padding.bottom as i8;
                        } else if let Some(frame) = &self.config.frame {
                            center_area_frame.inner_margin.top = frame.inner_margin.y as i8;
                            center_area_frame.inner_margin.bottom = frame.inner_margin.y as i8;
                        }

                        center_area_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
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

#[allow(clippy::too_many_arguments)]
fn handle_notification(
    ctx: &Context,
    event: komorebi_client::NotificationEvent,
    bg_color: Rc<RefCell<Color32>>,
    bg_color_with_alpha: Rc<RefCell<Color32>>,
    transparency_alpha: Option<u8>,
    grouping: Option<Grouping>,
    default_theme: Option<KomobarTheme>,
    render_config: Rc<RefCell<RenderConfig>>,
) {
    if let NotificationEvent::Socket(message) = event {
        match message {
            SocketMessage::ReloadStaticConfiguration(path) => {
                if let Ok(config) = komorebi_client::StaticConfig::read(&path) {
                    if let Some(theme) = config.theme {
                        apply_theme(
                            ctx,
                            KomobarTheme::from(theme),
                            bg_color.clone(),
                            bg_color_with_alpha.clone(),
                            transparency_alpha,
                            grouping,
                            render_config,
                        );
                        tracing::info!("applied theme from updated komorebi.json");
                    } else if let Some(default_theme) = default_theme {
                        apply_theme(
                            ctx,
                            default_theme,
                            bg_color.clone(),
                            bg_color_with_alpha.clone(),
                            transparency_alpha,
                            grouping,
                            render_config,
                        );
                        tracing::info!(
                            "removed theme from updated komorebi.json and applied default theme"
                        );
                    } else {
                        tracing::warn!("theme was removed from updated komorebi.json but there was no default theme to apply");
                    }
                }
            }
            SocketMessage::Theme(theme) => {
                apply_theme(
                    ctx,
                    KomobarTheme::from(*theme),
                    bg_color,
                    bg_color_with_alpha.clone(),
                    transparency_alpha,
                    grouping,
                    render_config,
                );
                tracing::info!("applied theme from komorebi socket message");
            }
            _ => {}
        }
    }
}
