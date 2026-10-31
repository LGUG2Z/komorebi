use crate::render::RenderConfig;
use crate::widgets::applications::ApplicationsConfig;
use crate::widgets::battery::BatteryConfig;
use crate::widgets::cpu::CpuConfig;
use crate::widgets::date::DateConfig;
use crate::widgets::komorebi_config::KomorebiConfig;
use crate::widgets::memory::MemoryConfig;
use crate::widgets::network_config::NetworkConfig;
use crate::widgets::platform_config::KeyboardConfig;
use crate::widgets::platform_config::MediaConfig;
use crate::widgets::platform_config::SystrayConfig;
use crate::widgets::storage::StorageConfig;
use crate::widgets::time::TimeConfig;
use crate::widgets::update::UpdateConfig;
use eframe::egui::Context;
use eframe::egui::Ui;
use serde::Deserialize;
use serde::Serialize;

pub trait BarWidget {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig);
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Widget configuration
pub enum WidgetConfig {
    /// Applications widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Applications"))]
    Applications(ApplicationsConfig),
    /// Battery widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Battery"))]
    Battery(BatteryConfig),
    /// CPU widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Cpu"))]
    Cpu(CpuConfig),
    /// Date widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Date"))]
    Date(DateConfig),
    /// Keyboard widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Keyboard"))]
    Keyboard(KeyboardConfig),
    /// Komorebi widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Komorebi"))]
    Komorebi(KomorebiConfig),
    /// Media widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Media"))]
    Media(MediaConfig),
    /// Memory widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Memory"))]
    Memory(MemoryConfig),
    /// Network widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Network"))]
    Network(NetworkConfig),
    /// Storage widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Storage"))]
    Storage(StorageConfig),
    /// System Tray widget configuration (Windows only)
    #[cfg_attr(feature = "schemars", schemars(title = "Systray"))]
    Systray(SystrayConfig),
    /// Time widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Time"))]
    Time(TimeConfig),
    /// Update widget configuration
    #[cfg_attr(feature = "schemars", schemars(title = "Update"))]
    Update(UpdateConfig),
}

impl WidgetConfig {
    /// The widget a configuration key names, if the platform this bar runs on has it. The
    /// keyboard, media and system tray widgets exist on Windows only: elsewhere the key is
    /// accepted and skipped (see [`WidgetConfig::unsupported`]).
    #[must_use]
    pub const fn supported(&self) -> bool {
        !matches!(
            self,
            WidgetConfig::Keyboard(_) | WidgetConfig::Media(_) | WidgetConfig::Systray(_)
        ) || cfg!(target_os = "windows")
    }

    /// The name of this widget when the platform cannot run it.
    #[must_use]
    pub const fn unsupported(&self) -> Option<&'static str> {
        if self.supported() {
            return None;
        }

        match self {
            WidgetConfig::Keyboard(_) => Some("Keyboard"),
            WidgetConfig::Media(_) => Some("Media"),
            WidgetConfig::Systray(_) => Some("Systray"),
            _ => None,
        }
    }

    /// Whether the widget is enabled and can run on this platform.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.supported()
            && match self {
                WidgetConfig::Applications(config) => config.enable,
                WidgetConfig::Battery(config) => config.enable,
                WidgetConfig::Cpu(config) => config.enable,
                WidgetConfig::Date(config) => config.enable,
                WidgetConfig::Keyboard(config) => config.enable,
                WidgetConfig::Komorebi(config) => {
                    config.workspaces.as_ref().is_some_and(|w| w.enable)
                        || config.layout.as_ref().is_some_and(|w| w.enable)
                        || config.focused_container.as_ref().is_some_and(|w| w.enable)
                        || config
                            .configuration_switcher
                            .as_ref()
                            .is_some_and(|w| w.enable)
                }
                WidgetConfig::Media(config) => config.enable,
                WidgetConfig::Memory(config) => config.enable,
                WidgetConfig::Network(config) => config.enable,
                WidgetConfig::Storage(config) => config.enable,
                WidgetConfig::Systray(config) => config.enable,
                WidgetConfig::Time(config) => config.enable,
                WidgetConfig::Update(config) => config.enable,
            }
    }
}

/// Logs a warning for every widget in `widgets` that this platform cannot run; they are accepted
/// in the configuration and skipped.
pub fn warn_unsupported<'a>(widgets: impl IntoIterator<Item = &'a WidgetConfig>) {
    for name in widgets.into_iter().filter_map(WidgetConfig::unsupported) {
        tracing::warn!("the {name} widget is not supported on this platform and is skipped");
    }
}
