use super::ICONS_CACHE;
use super::ImageIcon;
use super::ImageIconId;
use super::rgba_to_color_image;
use crate::bar::Alignment;
use crate::mark_widget_clicked;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_channel::unbounded;
use eframe::egui::CentralPanel;
use eframe::egui::ColorImage;
use eframe::egui::Context;
use eframe::egui::Frame;
use eframe::egui::Image;
use eframe::egui::Label;
use eframe::egui::Margin;
use eframe::egui::ScrollArea;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use eframe::egui::ViewportBuilder;
use eframe::egui::ViewportClass;
use eframe::egui::ViewportId;
use eframe::egui::Window as EguiWindow;
use egui_extras::Column;
use egui_extras::TableBuilder;
use komorebi_client::MatchingStrategy;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use systray_util::StableId;
use systray_util::Systray as SystrayClient;
use systray_util::SystrayEvent;
use systray_util::SystrayIconAction;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::OpenProcess;
use windows::Win32::System::Threading::PROCESS_NAME_WIN32;
use windows::Win32::System::Threading::PROCESS_QUERY_INFORMATION;
use windows::Win32::System::Threading::QueryFullProcessImageNameW;
use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;
use windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN;
use windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN;
use windows::core::PWSTR;

/// Whether hidden icons are currently shown
static SHOW_HIDDEN_ICONS: AtomicBool = AtomicBool::new(false);

/// Whether the systray info panel is currently shown
static SHOW_SYSTRAY_INFO: AtomicBool = AtomicBool::new(false);

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

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Where to place a systray button (refresh, info, shortcuts, etc.)
pub enum ButtonPosition {
    /// Show in the main visible area
    Visible,
    /// Show in the overflow/hidden section
    Overflow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// A field value with an optional matching strategy.
///
/// A plain string uses exact (case-insensitive) matching.
/// An object with `value` and `matching_strategy` uses the specified strategy.
pub enum FieldMatch {
    /// Exact case-insensitive match
    Exact(String),
    /// Match using a specific strategy
    WithStrategy {
        /// The value to match against
        value: String,
        /// How to match (Equals, StartsWith, EndsWith, Contains, Regex, etc.)
        matching_strategy: MatchingStrategy,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Rule for matching a systray icon to hide
///
/// A plain string matches the exe name (backward compatible).
/// An object with optional `exe`, `tooltip`, and/or `guid` fields
/// uses AND logic: all specified fields must match.
/// Each field can be a plain string (exact match) or an object with
/// `value` and `matching_strategy` for advanced matching.
pub enum HiddenIconRule {
    /// Match by exe name (case-insensitive, exact)
    Exe(String),
    /// Match by one or more properties (all specified fields must match)
    Match {
        /// Exe name to match
        exe: Option<FieldMatch>,
        /// Tooltip text to match
        tooltip: Option<FieldMatch>,
        /// Icon GUID to match (most stable identifier across restarts)
        guid: Option<FieldMatch>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// System tray widget configuration
pub struct SystrayConfig {
    /// Enable the System Tray widget
    pub enable: bool,
    /// A list of rules for icons to hide from the system tray
    ///
    /// Each entry can be a plain string (matches exe name, case-insensitive)
    /// or an object with optional `exe`, `tooltip`, and/or `guid` fields
    /// (all specified fields must match, AND logic, case-insensitive).
    ///
    /// Run komorebi-bar with RUST_LOG=info to see the properties of all
    /// systray icons in the log output.
    pub hidden_icons: Option<Vec<HiddenIconRule>>,
    /// Position of the overflow toggle button (default: Right)
    pub overflow_toggle_position: Option<OverflowTogglePosition>,
    /// Show an info button that opens a floating panel listing all systray icons
    /// with their exe, tooltip, and GUID. Set to "Visible" to show it in the main
    /// area, or "Overflow" to show it in the hidden/overflow section.
    pub info_button: Option<ButtonPosition>,
    /// Interval in seconds to automatically check for and remove stale icons
    /// whose owning process has exited. Clamped between 30 and 600 seconds.
    /// Defaults to 60. Set to 0 to disable.
    pub stale_icons_check_interval: Option<u64>,
    /// Show a refresh button that manually triggers stale icon cleanup.
    /// Set to "Visible" to show it in the main area, or "Overflow" to
    /// show it in the hidden/overflow section.
    pub refresh_button: Option<ButtonPosition>,
    /// Show a button that toggles komorebi-shortcuts (kills the process if
    /// running, starts it otherwise). Set to "Visible" to show it in the main
    /// area, or "Overflow" to show it in the hidden/overflow section.
    pub shortcuts_button: Option<ButtonPosition>,
}

/// Command sent from UI to background thread
#[derive(Debug)]
enum SystrayCommand {
    SendAction(StableId, SystrayIconAction),
}

/// A single field condition: pre-lowercased value (except for Regex),
/// the matching strategy, and an optional pre-compiled regex.
#[derive(Clone)]
struct NormalizedField {
    value: String,
    strategy: MatchingStrategy,
    compiled_regex: Option<regex::Regex>,
}

impl NormalizedField {
    /// Build from a raw value and strategy. Pre-lowercases the value for
    /// non-regex strategies and pre-compiles the regex pattern if applicable.
    fn new(value: String, strategy: MatchingStrategy) -> Self {
        let compiled_regex = if strategy == MatchingStrategy::Regex {
            match regex::Regex::new(&value) {
                Ok(re) => Some(re),
                Err(err) => {
                    tracing::warn!("invalid regex in hidden_icons rule: {err}");
                    None
                }
            }
        } else {
            None
        };

        let normalized_value = if strategy == MatchingStrategy::Regex {
            value
        } else {
            value.to_lowercase()
        };

        Self {
            value: normalized_value,
            strategy,
            compiled_regex,
        }
    }

    /// Build from a `FieldMatch` config value.
    fn from_field_match(field: FieldMatch) -> Self {
        match field {
            FieldMatch::Exact(s) => Self::new(s, MatchingStrategy::Equals),
            FieldMatch::WithStrategy {
                value,
                matching_strategy,
            } => Self::new(value, matching_strategy),
        }
    }

    /// Returns true if this field condition matches the given input string.
    /// Non-regex comparisons are case-insensitive (value is pre-lowercased,
    /// input is lowercased at match time).
    fn matches(&self, input: &str) -> bool {
        if self.strategy == MatchingStrategy::Regex {
            return self
                .compiled_regex
                .as_ref()
                .is_some_and(|re| re.is_match(input));
        }

        let input_lower = input.to_lowercase();
        match self.strategy {
            MatchingStrategy::Legacy | MatchingStrategy::Equals => self.value == input_lower,
            MatchingStrategy::StartsWith => input_lower.starts_with(&self.value),
            MatchingStrategy::EndsWith => input_lower.ends_with(&self.value),
            MatchingStrategy::Contains => input_lower.contains(&self.value),
            MatchingStrategy::DoesNotEqual => self.value != input_lower,
            MatchingStrategy::DoesNotStartWith => !input_lower.starts_with(&self.value),
            MatchingStrategy::DoesNotEndWith => !input_lower.ends_with(&self.value),
            MatchingStrategy::DoesNotContain => !input_lower.contains(&self.value),
            MatchingStrategy::Regex => unreachable!(),
        }
    }
}

/// Pre-normalized hidden icon matching rule with per-field matching strategies.
#[derive(Clone)]
struct NormalizedHiddenRule {
    exe: Option<NormalizedField>,
    tooltip: Option<NormalizedField>,
    guid: Option<NormalizedField>,
}

impl NormalizedHiddenRule {
    /// Returns true if this rule matches the given icon properties (AND logic).
    /// All specified (non-None) fields must match.
    fn matches(&self, exe_name: &str, tooltip: &str, guid: Option<&str>) -> bool {
        let exe_ok = self.exe.as_ref().is_none_or(|f| f.matches(exe_name));
        let tooltip_ok = self.tooltip.as_ref().is_none_or(|f| f.matches(tooltip));
        let guid_ok = self
            .guid
            .as_ref()
            .is_none_or(|f| guid.is_some_and(|actual| f.matches(actual)));
        exe_ok && tooltip_ok && guid_ok
    }
}

impl From<HiddenIconRule> for NormalizedHiddenRule {
    fn from(rule: HiddenIconRule) -> Self {
        match rule {
            HiddenIconRule::Exe(exe) => Self {
                exe: Some(NormalizedField::new(exe, MatchingStrategy::Equals)),
                tooltip: None,
                guid: None,
            },
            HiddenIconRule::Match { exe, tooltip, guid } => Self {
                exe: exe.map(NormalizedField::from_field_match),
                tooltip: tooltip.map(NormalizedField::from_field_match),
                guid: guid.map(NormalizedField::from_field_match),
            },
        }
    }
}

/// Cached icon data for display
#[derive(Clone, Debug)]
struct CachedIcon {
    stable_id: StableId,
    tooltip: String,
    exe_name: String,
    guid_str: Option<String>,
    window_handle: Option<isize>,
    image_icon: Option<ImageIcon>,
    is_visible: bool,
    /// Whether the icon has a callback message and can receive click events
    is_clickable: bool,
}

/// Pre-processed icon data from the background thread.
/// Image conversion (RgbaImage -> ColorImage) is done off the UI thread.
struct PreprocessedIcon {
    stable_id: StableId,
    tooltip: String,
    window_handle: Option<isize>,
    is_visible: bool,
    /// Whether the icon has a callback message and can receive click events
    is_clickable: bool,
    /// Stable ID as string (pre-computed to avoid repeated allocations)
    stable_id_str: String,
    /// Cache key for the icon image (stable_id + hash)
    image_cache_key: String,
    /// Pre-converted ColorImage (conversion done off UI thread)
    color_image: Option<Arc<ColorImage>>,
    /// GUID string for logging only
    guid_str: Option<String>,
}

/// Pre-processed event sent from background thread to UI
enum PreprocessedEvent {
    IconAddOrUpdate(PreprocessedIcon),
    IconRemove(StableId),
}

/// Global shared state for the systray (UI-side only, no SystrayClient here)
#[derive(Default)]
struct GlobalSystrayState {
    /// Cached icons for rendering (shared across all widget instances)
    icons: HashMap<String, CachedIcon>,
    /// Receiver for pre-processed events from the background thread
    event_rx: Option<Receiver<PreprocessedEvent>>,
    /// Sender for commands to the background thread
    command_tx: Option<Sender<SystrayCommand>>,
    /// Whether the background thread has been started
    initialized: bool,
    /// Last time stale icon cleanup was performed
    last_cleanup: Option<Instant>,
}

/// Global singleton for the systray state
static SYSTRAY_STATE: LazyLock<Arc<Mutex<GlobalSystrayState>>> =
    LazyLock::new(|| Arc::new(Mutex::new(GlobalSystrayState::default())));

/// Pre-process a SystrayEvent in the background thread.
/// Converts RgbaImage -> ColorImage so the UI thread only does the GPU upload.
fn preprocess_event(event: SystrayEvent) -> PreprocessedEvent {
    match event {
        SystrayEvent::IconAdd(ref icon) | SystrayEvent::IconUpdate(ref icon) => {
            let stable_id_str = icon.stable_id.to_string();

            let image_cache_key = match &icon.icon_image_hash {
                Some(hash) => format!("{}_{}", stable_id_str, hash),
                None => stable_id_str.clone(),
            };

            let guid_str = icon.guid.map(|g| g.to_string());

            // Convert RgbaImage -> ColorImage here (background thread) instead of the UI thread
            let color_image = icon
                .icon_image
                .as_ref()
                .map(|img| Arc::new(rgba_to_color_image(&img.clone())));

            let is_clickable = icon.window_handle.is_some()
                && icon.uid.is_some()
                && icon.callback_message.is_some();

            PreprocessedEvent::IconAddOrUpdate(PreprocessedIcon {
                stable_id: icon.stable_id.clone(),
                tooltip: icon.tooltip.clone(),
                window_handle: icon.window_handle,
                is_visible: icon.is_visible,
                is_clickable,
                stable_id_str,
                image_cache_key,
                color_image,
                guid_str,
            })
        }
        SystrayEvent::IconRemove(stable_id) => PreprocessedEvent::IconRemove(stable_id),
    }
}

/// Initialize the global systray background thread (only runs once)
fn ensure_systray_initialized() {
    let mut state = SYSTRAY_STATE.lock();

    if state.initialized {
        return;
    }

    state.initialized = true;

    let (event_tx, event_rx) = unbounded::<PreprocessedEvent>();
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

                    // Send initial icons (pre-processed in background thread)
                    for icon in systray.icons() {
                        let event = SystrayEvent::IconAdd(icon.clone());
                        let _ = event_tx.send(preprocess_event(event));
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
                            // Handle systray events (pre-process before sending)
                            event = systray.events() => {
                                match event {
                                    Some(event) => {
                                        if event_tx.send(preprocess_event(event)).is_err() {
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
    /// Pre-normalized rules for hiding icons
    hidden_icons: Vec<NormalizedHiddenRule>,
    /// Position of the overflow toggle button
    overflow_toggle_position: OverflowTogglePosition,
    /// Where to show the info button (None = no button)
    info_button: Option<ButtonPosition>,
    /// Interval for automatic stale icon cleanup (None = disabled)
    stale_icons_check_interval: Option<Duration>,
    /// Where to show the refresh button (None = no button)
    refresh_button: Option<ButtonPosition>,
    /// Where to show the shortcuts toggle button (None = no button)
    shortcuts_button: Option<ButtonPosition>,
}

impl From<&SystrayConfig> for Systray {
    fn from(value: &SystrayConfig) -> Self {
        // Initialize the global systray on first widget creation
        if value.enable {
            ensure_systray_initialized();
        }

        // Normalize rules (lowercase all fields) for case-insensitive matching
        let hidden_icons = value
            .hidden_icons
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(NormalizedHiddenRule::from)
            .collect();

        // 0 = disabled, None = default 60s, otherwise clamp to [30, 600]
        let stale_icons_check_interval = match value.stale_icons_check_interval {
            Some(0) => None,
            Some(secs) => Some(Duration::from_secs(secs.clamp(30, 600))),
            None => Some(Duration::from_secs(60)),
        };

        Self {
            enable: value.enable,
            hidden_icons,
            overflow_toggle_position: value.overflow_toggle_position.unwrap_or_default(),
            info_button: value.info_button,
            stale_icons_check_interval,
            refresh_button: value.refresh_button,
            shortcuts_button: value.shortcuts_button,
        }
    }
}

impl Systray {
    /// Process pending events from the background thread.
    /// Returns true if any events were processed.
    fn process_events() -> bool {
        // 1. Drain events from channel (brief lock)
        let events: Vec<PreprocessedEvent> = {
            let state = SYSTRAY_STATE.lock();
            match &state.event_rx {
                Some(rx) => rx.try_iter().collect(),
                None => return false,
            }
        };

        if events.is_empty() {
            return false;
        }

        // 2. Deduplicate — keep only the last event per stable_id.
        //    If an icon fires many updates between frames, only the last matters.
        let deduped = Self::deduplicate_events(events);

        // 3. Resolve exe names outside the lock.
        //    Exe names don't change for a given window handle, so we reuse
        //    any already-known name and only call the Win32 API for new icons.
        let known_exe_names: HashMap<String, String> = {
            let state = SYSTRAY_STATE.lock();
            state
                .icons
                .iter()
                .filter(|(_, icon)| !icon.exe_name.is_empty())
                .map(|(id, icon)| (id.clone(), icon.exe_name.clone()))
                .collect()
        };

        let mut resolved_exe_names: HashMap<String, String> = HashMap::new();
        for event in &deduped {
            if let PreprocessedEvent::IconAddOrUpdate(picon) = event
                && !known_exe_names.contains_key(&picon.stable_id_str)
                && !resolved_exe_names.contains_key(&picon.stable_id_str)
            {
                let exe_name = picon
                    .window_handle
                    .and_then(Self::get_exe_from_hwnd)
                    .unwrap_or_default();
                resolved_exe_names.insert(picon.stable_id_str.clone(), exe_name);
            }
        }

        // 4. Process deduplicated events with the lock held (fast path only —
        //    no Win32 calls or image conversion happen here).
        let mut state = SYSTRAY_STATE.lock();
        for event in deduped {
            match event {
                PreprocessedEvent::IconAddOrUpdate(picon) => {
                    let exe_name = known_exe_names
                        .get(&picon.stable_id_str)
                        .or_else(|| resolved_exe_names.get(&picon.stable_id_str))
                        .cloned()
                        .unwrap_or_default();

                    // Evict stale cache entry if the image hash changed
                    if let Some(old_cached) = state.icons.get(&picon.stable_id_str)
                        && let Some(old_icon) = &old_cached.image_icon
                    {
                        let new_key = ImageIconId::SystrayIcon(picon.image_cache_key.clone());
                        if old_icon.id != new_key {
                            ICONS_CACHE.remove(&old_icon.id);
                        }
                    }

                    let stable_id_str = picon.stable_id_str.clone();
                    let cached = Self::create_cached_icon(picon, exe_name);
                    state.icons.insert(stable_id_str, cached);
                }
                PreprocessedEvent::IconRemove(stable_id) => {
                    let key = stable_id.to_string();
                    // Evict cache for removed icons
                    if let Some(old_cached) = state.icons.get(&key)
                        && let Some(old_icon) = &old_cached.image_icon
                    {
                        ICONS_CACHE.remove(&old_icon.id);
                    }
                    state.icons.remove(&key);
                }
            }
        }

        true
    }

    /// Deduplicate events, keeping only the last event per stable_id.
    /// For rapid-fire icon updates, this collapses N events into 1.
    fn deduplicate_events(events: Vec<PreprocessedEvent>) -> Vec<PreprocessedEvent> {
        let mut last_event: HashMap<String, PreprocessedEvent> = HashMap::new();

        for event in events {
            let key = match &event {
                PreprocessedEvent::IconAddOrUpdate(picon) => picon.stable_id_str.clone(),
                PreprocessedEvent::IconRemove(stable_id) => stable_id.to_string(),
            };
            last_event.insert(key, event);
        }

        last_event.into_values().collect()
    }

    /// Create a CachedIcon from pre-processed data.
    /// Image conversion and exe name resolution are already done by the time
    /// this is called.
    fn create_cached_icon(picon: PreprocessedIcon, exe_name: String) -> CachedIcon {
        tracing::info!(
            "Systray icon: tooltip={:?}, exe={:?}, guid={:?}, stable_id={}, is_visible={}",
            picon.tooltip,
            exe_name,
            picon.guid_str,
            picon.stable_id_str,
            picon.is_visible
        );

        // Image already converted by the background thread —
        // just insert the ColorImage into the shared cache.
        let image_icon = picon.color_image.map(|color_image| {
            let id = ImageIconId::SystrayIcon(picon.image_cache_key.clone());
            ICONS_CACHE.insert_image(id.clone(), color_image.clone());
            ImageIcon::new(id, color_image)
        });

        CachedIcon {
            stable_id: picon.stable_id,
            tooltip: picon.tooltip,
            exe_name,
            guid_str: picon.guid_str,
            window_handle: picon.window_handle,
            image_icon,
            is_visible: picon.is_visible,
            is_clickable: picon.is_clickable,
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

    /// Get a snapshot of all icons (including those not marked as visible by the OS)
    fn get_all_icons() -> Vec<CachedIcon> {
        let state = SYSTRAY_STATE.lock();
        let mut icons: Vec<_> = state.icons.values().cloned().collect();
        icons.sort_by(|a, b| a.stable_id.to_string().cmp(&b.stable_id.to_string()));
        icons
    }

    /// Check if an icon should be hidden based on the configured rules
    fn is_icon_hidden(&self, icon: &CachedIcon) -> bool {
        self.hidden_icons
            .iter()
            .any(|rule| rule.matches(&icon.exe_name, &icon.tooltip, icon.guid_str.as_deref()))
    }

    /// Remove icons whose owning window no longer exists.
    /// Returns true if any stale icons were removed.
    fn cleanup_stale_icons() -> bool {
        let mut state = SYSTRAY_STATE.lock();

        let stale_ids: Vec<String> = state
            .icons
            .iter()
            .filter(|(_, icon)| {
                icon.window_handle
                    .is_some_and(|hwnd| unsafe { !IsWindow(Some(HWND(hwnd as *mut _))).as_bool() })
            })
            .map(|(id, _)| id.clone())
            .collect();

        if stale_ids.is_empty() {
            return false;
        }

        for id in &stale_ids {
            if let Some(old_cached) = state.icons.get(id)
                && let Some(old_icon) = &old_cached.image_icon
            {
                ICONS_CACHE.remove(&old_icon.id);
            }
            state.icons.remove(id);
        }

        tracing::info!("Removed {} stale systray icon(s)", stale_ids.len());
        true
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

        // Periodic stale icon cleanup
        if let Some(interval) = self.stale_icons_check_interval {
            let should_cleanup = {
                let mut state = SYSTRAY_STATE.lock();
                let now = Instant::now();
                let due = state
                    .last_cleanup
                    .is_none_or(|last| now.duration_since(last) >= interval);
                if due {
                    state.last_cleanup = Some(now);
                }
                due
            };

            if should_cleanup && Self::cleanup_stale_icons() {
                ctx.request_repaint();
            }
        }

        // Render the floating info window before the bar layout so it is not
        // clipped by the widget's allocated area.
        self.render_info_window(ctx);

        let icon_size = config.icon_font_id.size;
        let all_icons = Self::get_visible_icons();

        // Separate visible and hidden icons
        let (visible_icons, hidden_icons): (Vec<_>, Vec<_>) = all_icons
            .into_iter()
            .partition(|icon| !self.is_icon_hidden(icon));

        let show_hidden = SHOW_HIDDEN_ICONS.load(Ordering::SeqCst);
        let refresh_in_overflow = self.refresh_button == Some(ButtonPosition::Overflow);
        let info_in_overflow = self.info_button == Some(ButtonPosition::Overflow);
        let shortcuts_in_overflow = self.shortcuts_button == Some(ButtonPosition::Overflow);
        let has_overflow_content = !hidden_icons.is_empty()
            || refresh_in_overflow
            || info_in_overflow
            || shortcuts_in_overflow;

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
            if has_overflow_content && effective_toggle_left {
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

            // Render visible-area buttons (after icons)
            if self.refresh_button == Some(ButtonPosition::Visible) {
                Self::render_refresh_button(ui, ctx);
            }
            if self.info_button == Some(ButtonPosition::Visible) {
                Self::render_info_button(ui);
            }
            if self.shortcuts_button == Some(ButtonPosition::Visible) {
                Self::render_shortcuts_button(ui);
            }

            // Render toggle button on the right if configured (default)
            if has_overflow_content && !effective_toggle_left {
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

        // If expanded, show the hidden icons and overflow buttons
        if show_hidden {
            for cached_icon in hidden_icons {
                self.render_icon(ctx, ui, cached_icon, icon_size);
            }

            if self.refresh_button == Some(ButtonPosition::Overflow) {
                Self::render_refresh_button(ui, ctx);
            }
            if self.info_button == Some(ButtonPosition::Overflow) {
                Self::render_info_button(ui);
            }
            if self.shortcuts_button == Some(ButtonPosition::Overflow) {
                Self::render_shortcuts_button(ui);
            }
        }
    }

    /// Render the refresh button that triggers stale icon cleanup
    fn render_refresh_button(ui: &mut Ui, ctx: &Context) {
        let response = SelectableFrame::new(false)
            .show(ui, |ui| {
                ui.add(Label::new(egui_phosphor::regular::ARROWS_CLOCKWISE).selectable(false));
            })
            .on_hover_text_at_pointer("Refresh systray icons");

        if response.clicked() {
            mark_widget_clicked();
            if Self::cleanup_stale_icons() {
                ctx.request_repaint();
            }
        }
    }

    /// Render the info button that toggles the info panel
    fn render_info_button(ui: &mut Ui) {
        let show_info = SHOW_SYSTRAY_INFO.load(Ordering::SeqCst);

        let response = SelectableFrame::new(show_info)
            .show(ui, |ui| {
                ui.add(Label::new(egui_phosphor::regular::INFO).selectable(false));
            })
            .on_hover_text_at_pointer("Show systray icon details");

        if response.clicked() {
            mark_widget_clicked();
            SHOW_SYSTRAY_INFO.store(!show_info, Ordering::SeqCst);
        }
    }

    /// Render the shortcuts toggle button.
    /// Toggles `komorebi-shortcuts.exe`: kills it if running, starts it otherwise.
    fn render_shortcuts_button(ui: &mut Ui) {
        let response = SelectableFrame::new(false)
            .show(ui, |ui| {
                ui.add(Label::new(egui_phosphor::regular::KEYBOARD).selectable(false));
            })
            .on_hover_text_at_pointer("Toggle shortcuts");

        if response.clicked() {
            mark_widget_clicked();
            // Run on a background thread so `taskkill` output() doesn't block the UI
            thread::spawn(|| {
                let killed = std::process::Command::new("taskkill")
                    .args(["/F", "/IM", "komorebi-shortcuts.exe"])
                    .output()
                    .is_ok_and(|o| o.status.success());

                if !killed {
                    let _ = std::process::Command::new("komorebi-shortcuts.exe").spawn();
                }
            });
        }
    }

    /// Render the info panel as a separate OS window via a deferred viewport.
    /// This avoids being clipped by the bar's thin OS window.
    fn render_info_window(&self, ctx: &Context) {
        if !SHOW_SYSTRAY_INFO.load(Ordering::SeqCst) {
            return;
        }

        // Clone the rules into an Arc so the `Send + Sync + 'static` callback
        // can use them for the "Hidden (rule)" column.
        let rules: Arc<Vec<NormalizedHiddenRule>> = Arc::new(self.hidden_icons.clone());

        let window_size = [500.0f32, 300.0];
        // GetSystemMetrics returns physical pixels; ViewportBuilder expects
        // logical points, so divide by the current DPI scale factor.
        let scale = ctx.pixels_per_point();
        let center = unsafe {
            let sw = GetSystemMetrics(SM_CXSCREEN) as f32 / scale;
            let sh = GetSystemMetrics(SM_CYSCREEN) as f32 / scale;
            [(sw - window_size[0]) / 2.0, (sh - window_size[1]) / 2.0]
        };

        ctx.show_viewport_deferred(
            ViewportId::from_hash_of("systray_info"),
            ViewportBuilder::default()
                .with_title("Systray Icons")
                .with_inner_size(window_size)
                .with_position(center),
            move |ctx, class| {
                // Handle the OS window's close button
                if ctx.input(|i| i.viewport().close_requested()) {
                    SHOW_SYSTRAY_INFO.store(false, Ordering::SeqCst);
                    return;
                }

                match class {
                    ViewportClass::Embedded => {
                        // Fallback when the backend doesn't support multiple
                        // OS windows — render inside an egui::Window (clipped
                        // to the bar, but still functional).
                        let mut open = true;
                        EguiWindow::new("Systray Icons")
                            .open(&mut open)
                            .resizable(true)
                            .default_size([500.0, 300.0])
                            .show(ctx, |ui| {
                                Self::render_info_content(ui, ctx, &rules);
                            });
                        if !open {
                            SHOW_SYSTRAY_INFO.store(false, Ordering::SeqCst);
                        }
                    }
                    ViewportClass::Deferred | ViewportClass::Immediate | ViewportClass::Root => {
                        CentralPanel::default().show(ctx, |ui| {
                            Self::render_info_content(ui, ctx, &rules);
                        });
                    }
                }
            },
        );
    }

    /// Render a small copy-to-clipboard button
    fn copy_button(ui: &mut Ui, text: &str) {
        if ui
            .small_button(egui_phosphor::regular::COPY)
            .on_hover_text("Copy to clipboard")
            .clicked()
        {
            ui.ctx().copy_text(text.to_string());
        }
    }

    /// Render the info table content (shared between deferred and embedded viewport paths).
    /// Uses `egui_extras::TableBuilder` for striped rows with overlines.
    fn render_info_content(ui: &mut Ui, ctx: &Context, rules: &[NormalizedHiddenRule]) {
        let all_icons = Self::get_all_icons();

        let line_height = eframe::egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y);

        ScrollArea::horizontal().show(ui, |ui| {
            let available_height = ui.available_height();

            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(eframe::egui::Layout::left_to_right(
                    eframe::egui::Align::Center,
                ))
                .column(Column::auto()) // Icon
                .column(Column::auto().resizable(true)) // Exe
                .column(Column::auto().resizable(true)) // Tooltip
                .column(Column::auto().resizable(true)) // GUID
                .column(Column::auto()) // Visible
                .column(Column::auto().resizable(true)) // Clickable
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height);

            table
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("Icon");
                    });
                    header.col(|ui| {
                        ui.strong("Exe");
                    });
                    header.col(|ui| {
                        ui.strong("Tooltip");
                    });
                    header.col(|ui| {
                        ui.strong("GUID");
                    });
                    header.col(|ui| {
                        ui.strong("Visible");
                    });
                    header.col(|ui| {
                        ui.strong("Clickable");
                    });
                })
                .body(|mut body| {
                    for icon in &all_icons {
                        // Size the row to fit multi-line tooltips
                        let tooltip_lines = icon.tooltip.lines().count().max(1);
                        let row_height = line_height * tooltip_lines as f32;

                        body.row(row_height, |mut row| {
                            row.set_overline(true);

                            row.col(|ui| {
                                if let Some(image_icon) = &icon.image_icon {
                                    ui.add(
                                        Image::from_texture(&image_icon.texture(ctx))
                                            .maintain_aspect_ratio(true)
                                            .fit_to_exact_size(Vec2::splat(16.0)),
                                    );
                                } else {
                                    ui.allocate_space(Vec2::splat(16.0));
                                }
                            });
                            row.col(|ui| {
                                ui.label(&icon.exe_name);
                                if !icon.exe_name.is_empty() {
                                    Self::copy_button(ui, &icon.exe_name);
                                }
                            });
                            row.col(|ui| {
                                ui.label(&icon.tooltip);
                                if !icon.tooltip.is_empty() {
                                    Self::copy_button(ui, &icon.tooltip);
                                }
                            });
                            row.col(|ui| {
                                let guid = icon.guid_str.as_deref().unwrap_or("—");
                                ui.label(guid);
                                if icon.guid_str.is_some() {
                                    Self::copy_button(ui, guid);
                                }
                            });
                            row.col(|ui| {
                                let hidden_by_rule = rules.iter().any(|rule| {
                                    rule.matches(
                                        &icon.exe_name,
                                        &icon.tooltip,
                                        icon.guid_str.as_deref(),
                                    )
                                });
                                let visibility_text = if !icon.is_visible {
                                    "Hidden (OS)"
                                } else if hidden_by_rule {
                                    "Hidden (rule)"
                                } else {
                                    "Yes"
                                };
                                ui.label(visibility_text);
                            });
                            row.col(|ui| {
                                if let Some(cmd) =
                                    Self::fallback_command(&icon.exe_name, &icon.tooltip)
                                {
                                    ui.label(format!("Fallback: {cmd}"));
                                } else if icon.is_clickable {
                                    ui.label("Yes");
                                } else {
                                    ui.label("No");
                                }
                            });
                        });
                    }
                });
        });
    }

    /// Render a single systray icon
    fn render_icon(&self, ctx: &Context, ui: &mut Ui, cached_icon: &CachedIcon, icon_size: f32) {
        let stable_id = cached_icon.stable_id.clone();
        let tooltip = &cached_icon.tooltip;

        let response = SelectableFrame::new(false).show(ui, |ui| {
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
        });

        let response = if tooltip.is_empty() {
            response
        } else {
            response.on_hover_text_at_pointer(tooltip)
        };

        // Handle mouse clicks - mark as consumed to prevent bar from also handling.
        // Fallback commands take priority: if we've defined a fallback for an icon,
        // we know its native click is broken/useless (some icons register a callback
        // but don't actually respond to click messages).
        let has_fallback =
            Self::fallback_command(&cached_icon.exe_name, &cached_icon.tooltip).is_some();

        // Check double_clicked() before clicked() because egui fires both on
        // the second click of a double-click — we want to send only the
        // double-click action in that case.
        if response.double_clicked() {
            mark_widget_clicked();
            if has_fallback {
                Self::fallback_click(cached_icon);
            } else {
                Self::send_action(&stable_id, SystrayIconAction::LeftDoubleClick);
            }
        } else if response.clicked() {
            mark_widget_clicked();
            if has_fallback {
                Self::fallback_click(cached_icon);
            } else {
                Self::send_action(&stable_id, SystrayIconAction::LeftClick);
            }
        } else if response.secondary_clicked() {
            mark_widget_clicked();
            if has_fallback {
                Self::fallback_click(cached_icon);
            } else {
                Self::send_action(&stable_id, SystrayIconAction::RightClick);
            }
        } else if response.middle_clicked() {
            mark_widget_clicked();
            if has_fallback {
                Self::fallback_click(cached_icon);
            } else {
                Self::send_action(&stable_id, SystrayIconAction::MiddleClick);
            }
        }
    }

    /// Returns the fallback command for an icon with known broken/missing click
    /// handling, if one is defined. Fallbacks take priority over native clicks.
    fn fallback_command(exe_name: &str, tooltip: &str) -> Option<&'static str> {
        match exe_name.to_lowercase().as_str() {
            "securityhealthsystray.exe" => Some("start windowsdefender://"),
            "explorer.exe" if tooltip.ends_with('%') => Some("start ms-settings:apps-volume"),
            "explorer.exe" if tooltip.is_empty() => Some("start ms-settings:batterysaver"),
            _ => None,
        }
    }

    /// Execute the fallback command for an icon with broken/missing click handling.
    fn fallback_click(icon: &CachedIcon) {
        if let Some(cmd) = Self::fallback_command(&icon.exe_name, &icon.tooltip) {
            tracing::debug!(
                "Fallback click for non-clickable icon: exe={}, cmd={}",
                icon.exe_name,
                cmd
            );
            std::process::Command::new("cmd.exe")
                .args(["/C", cmd])
                .spawn()
                .ok();
        } else {
            tracing::debug!("No fallback for non-clickable icon: exe={}", icon.exe_name);
        }
    }
}
