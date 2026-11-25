use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

/// Which way the view goes when the strip of the Scrolling layout is panned: to the left is to
/// see what is to the left of the screen, which is to move the strip to the right
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum PanDirection {
    Left,
    Right,
}

impl PanDirection {
    /// How far the strip is moved on the screen to pan `distance` pixels this way, in pixels,
    /// to the right if it is positive
    #[must_use]
    pub fn strip_offset(self, distance: u32) -> f64 {
        match self {
            Self::Left => f64::from(distance),
            Self::Right => -f64::from(distance),
        }
    }
}

/// What the fingers of a gesture that pans the strip do, as far as the strip is concerned
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum PanGesture {
    /// The fingers have landed
    Begin,
    /// The fingers moved by `dx` and `dy` centimetres on the touchpad since the last time, to
    /// the right and down if they are positive, at `t` milliseconds on a clock that only the
    /// differences between the times of a gesture mean anything on
    Move { dx: f64, dy: f64, t: f64 },
    /// The fingers lifted
    End,
    /// The gesture was given up: the strip goes back to where it was
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn the_strip_goes_the_other_way_to_the_view() {
        assert!((PanDirection::Left.strip_offset(400) - 400.0).abs() < f64::EPSILON);
        assert!((PanDirection::Right.strip_offset(400) + 400.0).abs() < f64::EPSILON);
        assert!(PanDirection::Left.strip_offset(0).abs() < f64::EPSILON);
    }

    #[test]
    fn directions_have_the_names_that_the_others_have() {
        assert_eq!(
            <PanDirection as FromStr>::from_str("Left").unwrap(),
            PanDirection::Left
        );
        assert_eq!(PanDirection::Right.to_string(), "Right");
        assert!(<PanDirection as FromStr>::from_str("Up").is_err());
    }
}
