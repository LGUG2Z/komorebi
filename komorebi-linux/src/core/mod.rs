//! The vocabulary of the window manager: the enums and small structs that configuration,
//! commands and the layout code are written in.
//!
//! The layout types ([`DefaultLayout`], [`Rect`], [`OperationDirection`] and the rest) are
//! those of `komorebi-layouts`, re-exported here so that the rest of the crate has one place to
//! import from. [`pathext`] resolves environment variables in configured paths and
//! [`config_generation`] holds the rules that match applications by what they are called.
//!
//! [`SocketMessage`] is every message the daemon takes on its socket, [`StateQuery`] every
//! question [`SocketMessage::Query`] can ask, and [`SubscribeOptions`] what a subscriber can ask
//! of its notifications. A message that goes somewhere takes the directions of the layouts: an
//! [`OperationDirection`] is a side of the focused tile, and a [`CycleDirection`] the next or the
//! previous container in the order of the workspace.

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
pub use komorebi_config::placement::InitialWindowPlacementRules;
pub use komorebi_config::placement::PlacementMatchingRules;
pub use komorebi_config::placement::PlacementTarget;
pub use komorebi_config::placement::WindowPlacement;
pub use komorebi_protocol::pathext;
pub use komorebi_protocol::types::ApplicationIdentifier;
pub use komorebi_protocol::types::FocusFollowsMouseImplementation;
pub use komorebi_protocol::types::MonocleFocusBehaviour;
pub use komorebi_protocol::types::MoveBehaviour;
pub use komorebi_protocol::types::OperationBehaviour;
pub use komorebi_protocol::types::StateQuery;
pub use komorebi_protocol::types::SubscribeOptions;
pub use komorebi_protocol::types::WindowKind;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct WindowManagementBehaviour {
    /// The current WindowContainerBehaviour to be used
    pub current_behaviour: WindowContainerBehaviour,
    /// Override of `current_behaviour` to open new windows as floating windows
    /// that can be later toggled to tiled, when false it will default to
    /// `current_behaviour` again.
    pub float_override: bool,
    /// Determines if a new window should be spawned floating when on the floating layer and the
    /// floating layer behavior is set to float. This value is always calculated when checking for
    /// the management behavior on a specific workspace.
    pub floating_layer_override: bool,
    /// The floating layer behavior to be used if the float override is being used
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
    fn placement_target_accepts_a_number_or_a_string() {
        let parse = |json: &str| serde_json::from_str::<InitialWindowPlacementRules>(json);

        let index = InitialWindowPlacementRules::Target(PlacementTarget::Index(2));
        assert_eq!(parse("2").unwrap(), index);
        assert_eq!(parse(r#""2""#).unwrap(), index);
        assert_eq!(
            parse(r#""Primary""#).unwrap(),
            InitialWindowPlacementRules::Target(PlacementTarget::Placement(
                WindowPlacement::Primary
            ))
        );

        assert!(parse("-1").is_err());
        assert!(parse("1.5").is_err());
        assert!(parse("true").is_err());

        // the keys of the map form are always strings
        let Ok(InitialWindowPlacementRules::Rules(rules)) =
            parse(r#"{"2": {"kind": "Exe", "id": "a.exe"}, "Primary": []}"#)
        else {
            panic!("expected the map form");
        };
        assert!(rules.contains_key(&PlacementTarget::Index(2)));
        assert!(rules.contains_key(&PlacementTarget::Placement(WindowPlacement::Primary)));
    }
}
