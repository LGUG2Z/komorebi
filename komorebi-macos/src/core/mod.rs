use serde::Deserialize;
use serde::Serialize;

// Re-export types from komorebi-layouts
pub use komorebi_layouts::Arrangement;
pub use komorebi_layouts::Axis;
pub use komorebi_layouts::CycleDirection;
pub use komorebi_layouts::DEFAULT_RATIO;
pub use komorebi_layouts::DEFAULT_SECONDARY_RATIO;
pub use komorebi_layouts::DefaultLayout;
pub use komorebi_layouts::Direction;
pub use komorebi_layouts::GridLayoutOptions;
pub use komorebi_layouts::Layout;
pub use komorebi_layouts::LayoutDefaultEntry;
pub use komorebi_layouts::LayoutOptions;
pub use komorebi_layouts::MAX_RATIO;
pub use komorebi_layouts::MAX_RATIOS;
pub use komorebi_layouts::MIN_RATIO;
pub use komorebi_layouts::OperationDirection;
pub use komorebi_layouts::PanDirection;
pub use komorebi_layouts::PanGesture;
pub use komorebi_layouts::Rect;
pub use komorebi_layouts::ScrollingLayoutOptions;
pub use komorebi_layouts::Sizing;
pub use komorebi_layouts::scrolling::ScrollingCentering;
pub use komorebi_layouts::validate_ratios;

// Keep local modules that aren't in komorebi-layouts
pub mod animation;
pub mod asc;
pub mod config_generation;
pub use komorebi_config::behaviour::CrossBoundaryBehaviour;
pub use komorebi_config::behaviour::FloatingLayerBehaviour;
pub use komorebi_config::behaviour::Placement;
pub use komorebi_config::behaviour::WindowContainerBehaviour;
pub use komorebi_config::behaviour::WindowHidingPosition;
pub use komorebi_protocol::pathext;
pub use komorebi_protocol::types::ApplicationIdentifier;
pub use komorebi_protocol::types::MonocleFocusBehaviour;
pub use komorebi_protocol::types::MoveBehaviour;
pub use komorebi_protocol::types::OperationBehaviour;
pub use komorebi_protocol::types::StateQuery;
pub use komorebi_protocol::types::SubscribeOptions;
pub use komorebi_protocol::types::WindowKind;
pub mod rect_ext;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct WindowManagementBehaviour {
    /// The current WindowContainerBehaviour to be used
    pub current_behaviour: WindowContainerBehaviour,
    /// Override of `current_behaviour` to open new windows as floating windows
    /// that can be later toggled to tiled, when false it will default to
    /// `current_behaviour` again.
    pub float_override: bool,
    /// Determines if a new window should be spawned floating when on the floating layer and the
    /// floating layer behaviour is set to float. This value is always calculated when checking for
    /// the management behaviour on a specific workspace.
    pub floating_layer_override: bool,
    /// The floating layer behaviour to be used if the float override is being used
    pub floating_layer_behaviour: FloatingLayerBehaviour,
    /// The `Placement` to be used when toggling a window to float
    pub toggle_float_placement: Placement,
    /// The `Placement` to be used when spawning a window on the floating layer with the
    /// `FloatingLayerBehaviour` set to `FloatingLayerBehaviour::Float`
    pub floating_layer_placement: Placement,
    /// The `Placement` to be used when spawning a window with float override active
    pub float_override_placement: Placement,
    /// The `Placement` to be used when spawning a window that matches a 'floating_applications' rule
    pub float_rule_placement: Placement,
}

pub use komorebi_protocol::message::SocketMessage;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scrolling_command_goes_over_the_socket_as_it_is() {
        for message in [
            SocketMessage::ScrollingLayoutFullWidth,
            SocketMessage::CenterFocus,
            SocketMessage::Expand,
            SocketMessage::Pan(PanDirection::Left),
            SocketMessage::Consume(OperationDirection::Left),
            SocketMessage::Expel,
            SocketMessage::ToggleColumnStack,
        ] {
            let bytes = message.as_bytes().unwrap();
            let read: SocketMessage = String::from_utf8(bytes.clone()).unwrap().parse().unwrap();

            assert_eq!(read.as_bytes().unwrap(), bytes);
        }
    }
}
