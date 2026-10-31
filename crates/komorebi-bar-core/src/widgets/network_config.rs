//! The configuration of the network widget: data only, shared by both bars. The Windows bar
//! reads `default_interface_options`; the macOS bar accepts the key and ignores it.

use crate::config::LabelPrefix;
use serde::Deserialize;
use serde::Serialize;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Network widget configuration
pub struct NetworkConfig {
    /// Enable the Network widget
    pub enable: bool,
    /// Show total received and transmitted activity
    #[serde(alias = "show_total_data_transmitted")]
    pub show_total_activity: bool,
    /// Show received and transmitted activity
    #[serde(alias = "show_network_activity")]
    pub show_activity: bool,
    /// Show default interface
    pub show_default_interface: Option<bool>,
    /// Default interface options: whether to show the friendly name, IPv4 or IPv6 address
    pub default_interface_options: Option<DefaultInterfaceConfig>,
    /// Characters to reserve for received and transmitted activity
    #[serde(alias = "network_activity_fill_characters")]
    pub activity_left_padding: Option<usize>,
    /// Data refresh interval in seconds
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 10)))]
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
    /// Select when the value is over a limit (1MiB is 1048576 bytes (1024*1024))
    pub auto_select: Option<NetworkSelectConfig>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Network select configuration
pub struct NetworkSelectConfig {
    /// Select the total received data when it's over this value
    pub total_received_over: Option<u64>,
    /// Select the total transmitted data when it's over this value
    pub total_transmitted_over: Option<u64>,
    /// Select the received data when it's over this value
    pub received_over: Option<u64>,
    /// Select the transmitted data when it's over this value
    pub transmitted_over: Option<u64>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Which IP address to show for the default interface
pub enum InterfaceDisplayType {
    #[default]
    FriendlyName,
    Ipv4,
    Ipv6,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Default interface configuration
pub struct DefaultInterfaceConfig {
    #[serde(default)]
    pub display_type: InterfaceDisplayType,
}
