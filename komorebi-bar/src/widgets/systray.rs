use super::ImageIcon;
use super::ImageIconId;
use crate::bar::Alignment;
use crate::mark_widget_clicked;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_channel::unbounded;
use eframe::egui::Context;
use eframe::egui::Frame;
use eframe::egui::Image;
use eframe::egui::Label;
use eframe::egui::Margin;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use systray_util::StableId;
use systray_util::Systray as SystrayClient;
use systray_util::SystrayEvent;
use systray_util::SystrayIcon;
use systray_util::SystrayIconAction;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::OpenProcess;
use windows::Win32::System::Threading::PROCESS_NAME_WIN32;
use windows::Win32::System::Threading::PROCESS_QUERY_INFORMATION;
use windows::Win32::System::Threading::QueryFullProcessImageNameW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::core::PWSTR;

/// Whether hidden icons are currently shown
static SHOW_HIDDEN_ICONS: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Position of the overflow toggle button
pub enum OverflowTogglePosition {
    /// Toggle button appears on the left side (before visible icons)
    Left,
    /// Toggle button appears on the right side (after visible icons)
    #[default]
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// System tray widget configuration
pub struct SystrayConfig {
    /// Enable the System Tray widget
    pub enable: bool,
    /// A list of exe names to hide from the system tray
    ///
    /// Matching is case-insensitive. Run komorebi-bar with RUST_LOG=info to see
    /// the exe names of all systray icons in the log output.
    pub hidden_icons: Option<Vec<String>>,
    /// Position of the overflow toggle button (default: Right)
    pub overflow_toggle_position: Option<OverflowTogglePosition>,
    /// Show exe names as labels next to icons (useful for identifying apps to filter)
    pub show_exe_labels: Option<bool>,
}

/// Command sent from UI to background thread
#[derive(Debug)]
enum SystrayCommand {
    SendAction(StableId, SystrayIconAction),
}

/// Cached icon data for display
#[derive(Clone, Debug)]
struct CachedIcon {
    stable_id: StableId,
    tooltip: String,
    exe_name: String,
    image_icon: Option<ImageIcon>,
    is_visible: bool,
}

/// Global shared state for the systray (UI-side only, no SystrayClient here)
#[derive(Default)]
struct GlobalSystrayState {
    /// Cached icons for rendering (shared across all widget instances)
    icons: HashMap<String, CachedIcon>,
    /// Receiver for systray events from the background thread
    event_rx: Option<Receiver<SystrayEvent>>,
    /// Sender for commands to the background thread
    command_tx: Option<Sender<SystrayCommand>>,
    /// Whether the background thread has been started
    initialized: bool,
}

/// Global singleton for the systray state
static SYSTRAY_STATE: LazyLock<Arc<Mutex<GlobalSystrayState>>> =
    LazyLock::new(|| Arc::new(Mutex::new(GlobalSystrayState::default())));

/// Initialize the global systray background thread (only runs once)
fn ensure_systray_initialized() {
    let mut state = SYSTRAY_STATE.lock();

    if state.initialized {
        return;
    }

    state.initialized = true;

    let (event_tx, event_rx) = unbounded::<SystrayEvent>();
    let (command_tx, command_rx) = unbounded::<SystrayCommand>();

    state.event_rx = Some(event_rx);
    state.command_tx = Some(command_tx);

    // Drop the lock before spawning the thread
    drop(state);

    // Spawn background thread with its own tokio runtime
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for systray");

        rt.block_on(async move {
            match SystrayClient::new() {
                Ok(mut systray) => {
                    tracing::info!("Systray client initialized successfully");

                    // Send initial icons
                    for icon in systray.icons() {
                        let _ = event_tx.send(SystrayEvent::IconAdd(icon.clone()));
                    }

                    // Create an async channel receiver for commands
                    let (async_cmd_tx, mut async_cmd_rx) =
                        tokio::sync::mpsc::unbounded_channel::<SystrayCommand>();

                    // Spawn a task to bridge crossbeam -> tokio channel
                    let bridge_tx = async_cmd_tx.clone();
                    std::thread::spawn(move || {
                        while let Ok(cmd) = command_rx.recv() {
                            if bridge_tx.send(cmd).is_err() {
                                break;
                            }
                        }
                    });

                    loop {
                        tokio::select! {
                            // Handle systray events
                            event = systray.events() => {
                                match event {
                                    Some(event) => {
                                        if event_tx.send(event).is_err() {
                                            tracing::error!("Failed to send systray event to UI");
                                            break;
                                        }
                                    }
                                    None => {
                                        tracing::info!("Systray events channel closed");
                                        break;
                                    }
                                }
                            }
                            // Handle commands from UI
                            Some(cmd) = async_cmd_rx.recv() => {
                                match cmd {
                                    SystrayCommand::SendAction(stable_id, action) => {
                                        tracing::debug!(
                                            "Processing systray action for {}: {:?}",
                                            stable_id,
                                            action
                                        );
                                        if let Err(e) = systray.send_action(&stable_id, &action) {
                                            tracing::error!(
                                                "Failed to send systray action to {}: {:?}",
                                                stable_id,
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to initialize systray client: {:?}", e);
                }
            }
        });
    });
}

pub struct Systray {
    pub enable: bool,
    /// Lowercased exe names to hide
    hidden_icons: Vec<String>,
    /// Position of the overflow toggle button
    overflow_toggle_position: OverflowTogglePosition,
    /// Show exe names as labels next to icons
    show_exe_labels: bool,
}

impl From<&SystrayConfig> for Systray {
    fn from(value: &SystrayConfig) -> Self {
        // Initialize the global systray on first widget creation
        if value.enable {
            ensure_systray_initialized();
        }

        // Store lowercased exe names for case-insensitive matching
        let hidden_icons = value
            .hidden_icons
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect();

        Self {
            enable: value.enable,
            hidden_icons,
            overflow_toggle_position: value.overflow_toggle_position.unwrap_or_default(),
            show_exe_labels: value.show_exe_labels.unwrap_or(false),
        }
    }
}

impl Systray {
    /// Process pending events from the background thread
    /// Returns true if any events were processed
    fn process_events() -> bool {
        // First, collect all events while holding the lock briefly
        let events: Vec<SystrayEvent> = {
            let state = SYSTRAY_STATE.lock();
            match &state.event_rx {
                Some(rx) => rx.try_iter().collect(),
                None => return false,
            }
        };

        // Now process the events with a fresh lock
        if !events.is_empty() {
            let mut state = SYSTRAY_STATE.lock();
            for event in events {
                match event {
                    SystrayEvent::IconAdd(icon) | SystrayEvent::IconUpdate(icon) => {
                        let stable_id_str = icon.stable_id.to_string();
                        let cached = Self::create_cached_icon(&icon);
                        state.icons.insert(stable_id_str, cached);
                    }
                    SystrayEvent::IconRemove(stable_id) => {
                        state.icons.remove(&stable_id.to_string());
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Create a cached icon from a systray icon
    fn create_cached_icon(icon: &SystrayIcon) -> CachedIcon {
        let stable_id_str = icon.stable_id.to_string();

        // Try to get the exe name from the window handle for better filtering options
        let exe_name = icon
            .window_handle
            .and_then(Self::get_exe_from_hwnd)
            .unwrap_or_default();

        // Log all available icon information for debugging/filtering purposes
        // The GUID is the most stable identifier across restarts
        tracing::info!(
            "Systray icon: tooltip={:?}, exe={:?}, guid={:?}, stable_id={}, is_visible={}",
            icon.tooltip,
            exe_name,
            icon.guid.map(|g| g.to_string()),
            stable_id_str,
            icon.is_visible
        );

        // Use icon_image_hash to create a unique cache key that changes when the icon changes
        let cache_key = match &icon.icon_image_hash {
            Some(hash) => format!("{}_{}", stable_id_str, hash),
            None => stable_id_str.clone(),
        };

        let image_icon = icon.icon_image.as_ref().and_then(|icon_image| {
            ImageIcon::try_load(ImageIconId::SystrayIcon(cache_key), || {
                Some(icon_image.clone())
            })
        });

        CachedIcon {
            stable_id: icon.stable_id.clone(),
            tooltip: icon.tooltip.clone(),
            exe_name,
            image_icon,
            is_visible: icon.is_visible,
        }
    }

    /// Get the executable name from a window handle
    fn get_exe_from_hwnd(hwnd: isize) -> Option<String> {
        unsafe {
            let mut process_id: u32 = 0;
            GetWindowThreadProcessId(
                HWND(hwnd as *mut _),
                Some(std::ptr::addr_of_mut!(process_id)),
            );

            if process_id == 0 {
                return None;
            }

            let handle = OpenProcess(PROCESS_QUERY_INFORMATION, false, process_id).ok()?;

            let mut len = 260_u32;
            let mut path: Vec<u16> = vec![0; len as usize];
            let text_ptr = path.as_mut_ptr();

            let result =
                QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(text_ptr), &mut len);

            let _ = CloseHandle(handle);

            if result.is_err() {
                return None;
            }

            let exe_path = String::from_utf16(&path[..len as usize]).ok()?;

            // Extract just the filename from the path
            exe_path.rsplit('\\').next().map(|s| s.to_string())
        }
    }

    /// Send a click action to an icon
    fn send_action(stable_id: &StableId, action: SystrayIconAction) {
        let state = SYSTRAY_STATE.lock();

        if let Some(command_tx) = &state.command_tx
            && command_tx
                .send(SystrayCommand::SendAction(stable_id.clone(), action))
                .is_err()
        {
            tracing::error!("Failed to send command to systray thread");
        }
    }

    /// Get a snapshot of current icons
    fn get_visible_icons() -> Vec<CachedIcon> {
        let state = SYSTRAY_STATE.lock();
        let mut icons: Vec<_> = state
            .icons
            .values()
            .filter(|icon| icon.is_visible)
            .cloned()
            .collect();
        icons.sort_by(|a, b| a.stable_id.to_string().cmp(&b.stable_id.to_string()));
        icons
    }

    /// Check if an icon should be hidden based on its exe name
    fn is_icon_hidden(&self, exe_name: &str) -> bool {
        let exe_lower = exe_name.to_lowercase();
        self.hidden_icons.contains(&exe_lower)
    }
}

impl BarWidget for Systray {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if !self.enable {
            return;
        }

        // Process any pending events and request repaint if there were any
        if Self::process_events() {
            ctx.request_repaint();
        }

        let icon_size = config.icon_font_id.size;
        let all_icons = Self::get_visible_icons();

        // Separate visible and hidden icons
        let (visible_icons, hidden_icons): (Vec<_>, Vec<_>) = all_icons
            .into_iter()
            .partition(|icon| !self.is_icon_hidden(&icon.exe_name));

        let show_hidden = SHOW_HIDDEN_ICONS.load(Ordering::SeqCst);
        let has_hidden_icons = !hidden_icons.is_empty();

        // Check if we're in a right-aligned context (rendering is reversed)
        let is_reversed = matches!(config.alignment, Some(Alignment::Right));

        // Determine effective toggle position (flip if reversed)
        let effective_toggle_left = match (self.overflow_toggle_position, is_reversed) {
            (OverflowTogglePosition::Left, false) => true,
            (OverflowTogglePosition::Right, false) => false,
            (OverflowTogglePosition::Left, true) => false, // Flip when reversed
            (OverflowTogglePosition::Right, true) => true, // Flip when reversed
        };

        config.apply_on_widget(false, ui, |ui| {
            // Render toggle button on the left if configured
            if has_hidden_icons && effective_toggle_left {
                self.render_toggle_button(
                    ui,
                    show_hidden,
                    &hidden_icons,
                    icon_size,
                    ctx,
                    is_reversed,
                );
            }

            // Render visible icons
            for cached_icon in &visible_icons {
                self.render_icon(ctx, ui, cached_icon, icon_size);
            }

            // Render toggle button on the right if configured (default)
            if has_hidden_icons && !effective_toggle_left {
                self.render_toggle_button(
                    ui,
                    show_hidden,
                    &hidden_icons,
                    icon_size,
                    ctx,
                    is_reversed,
                );
            }
        });
    }
}

impl Systray {
    /// Render the toggle button for showing/hiding overflow icons
    fn render_toggle_button(
        &self,
        ui: &mut Ui,
        show_hidden: bool,
        hidden_icons: &[CachedIcon],
        icon_size: f32,
        ctx: &Context,
        is_reversed: bool,
    ) {
        // Determine arrow direction:
        // - When collapsed: arrow points toward where hidden icons will appear
        // - When expanded: arrow points toward visible icons (away from hidden icons)
        //
        // In left_widgets (not reversed) with toggle on Left:
        //   Collapsed: ? [visible...]  (arrow points left, where hidden will appear)
        //   Expanded:  ? [hidden...] [visible...]  (arrow points right, toward visible)
        //
        // In right_widgets (reversed) with toggle on Left:
        //   Collapsed: [visible...] ?  (arrow points left, where hidden will appear)
        //   Expanded:  [visible...] ? [hidden...]  (arrow points right, toward visible)
        let toggle_icon = match (self.overflow_toggle_position, is_reversed, show_hidden) {
            // Left position, normal rendering
            (OverflowTogglePosition::Left, false, false) => egui_phosphor::regular::CARET_LEFT,
            (OverflowTogglePosition::Left, false, true) => egui_phosphor::regular::CARET_RIGHT,
            // Right position, normal rendering
            (OverflowTogglePosition::Right, false, false) => egui_phosphor::regular::CARET_RIGHT,
            (OverflowTogglePosition::Right, false, true) => egui_phosphor::regular::CARET_LEFT,
            // Left position, reversed rendering (right_widgets) - arrows flipped
            (OverflowTogglePosition::Left, true, false) => egui_phosphor::regular::CARET_LEFT,
            (OverflowTogglePosition::Left, true, true) => egui_phosphor::regular::CARET_RIGHT,
            // Right position, reversed rendering (right_widgets) - arrows flipped
            (OverflowTogglePosition::Right, true, false) => egui_phosphor::regular::CARET_RIGHT,
            (OverflowTogglePosition::Right, true, true) => egui_phosphor::regular::CARET_LEFT,
        };

        let toggle_response = SelectableFrame::new(show_hidden)
            .show(ui, |ui| {
                ui.add(Label::new(toggle_icon).selectable(false));
            })
            .on_hover_text_at_pointer(if show_hidden {
                "Hide overflow icons"
            } else {
                "Show overflow icons"
            });

        if toggle_response.clicked() {
            mark_widget_clicked();
            SHOW_HIDDEN_ICONS.store(!show_hidden, Ordering::SeqCst);
        }

        // If expanded, show the hidden icons
        if show_hidden {
            for cached_icon in hidden_icons {
                self.render_icon(ctx, ui, cached_icon, icon_size);
            }
        }
    }

    /// Render a single systray icon
    fn render_icon(&self, ctx: &Context, ui: &mut Ui, cached_icon: &CachedIcon, icon_size: f32) {
        let stable_id = cached_icon.stable_id.clone();
        let tooltip = &cached_icon.tooltip;
        let exe_name = &cached_icon.exe_name;

        let response = SelectableFrame::new(false)
            .show(ui, |ui| {
                if let Some(image_icon) = &cached_icon.image_icon {
                    Frame::NONE
                        .inner_margin(Margin::same(ui.style().spacing.button_padding.y as i8))
                        .show(ui, |ui| {
                            ui.add(
                                Image::from_texture(&image_icon.texture(ctx))
                                    .maintain_aspect_ratio(true)
                                    .fit_to_exact_size(Vec2::splat(icon_size)),
                            );
                        });
                } else {
                    // Fallback: allocate space with a placeholder
                    ui.allocate_space(Vec2::splat(icon_size));
                }

                // Show exe label if enabled
                if self.show_exe_labels && !exe_name.is_empty() {
                    ui.add(Label::new(exe_name).selectable(false));
                }
            })
            .on_hover_text_at_pointer(tooltip);

        // Handle mouse clicks - mark as consumed to prevent bar from also handling
        if response.clicked() {
            mark_widget_clicked();
            Self::send_action(&stable_id, SystrayIconAction::LeftClick);
        } else if response.secondary_clicked() {
            mark_widget_clicked();
            Self::send_action(&stable_id, SystrayIconAction::RightClick);
        } else if response.middle_clicked() {
            mark_widget_clicked();
            Self::send_action(&stable_id, SystrayIconAction::MiddleClick);
        }
    }
}
