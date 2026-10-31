//! The easing curves an animation can have (`komorebic animation-style`).

pub mod prefix;

use clap::ValueEnum;

use serde::Deserialize;
use serde::Serialize;
use serde::ser::SerializeSeq;
use strum::Display;
use strum::EnumString;

#[derive(Copy, Clone, Debug, Display, EnumString, ValueEnum, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Mathematical function which describes the rate at which a value changes
pub enum AnimationStyle {
    /// Linear
    Linear,
    /// Ease in sine
    EaseInSine,
    /// Ease out sine
    EaseOutSine,
    /// Ease in out sine
    EaseInOutSine,
    /// Ease in quad
    EaseInQuad,
    /// Ease out quad
    EaseOutQuad,
    /// Ease in out quad
    EaseInOutQuad,
    /// Ease in cubic
    EaseInCubic,
    /// Ease out cubic
    EaseOutCubic,
    /// Ease in out cubic
    EaseInOutCubic,
    /// Ease in quart
    EaseInQuart,
    /// Ease out quart
    EaseOutQuart,
    /// Ease in out quart
    EaseInOutQuart,
    /// Ease in quint
    EaseInQuint,
    /// Ease out quint
    EaseOutQuint,
    /// Ease in out quint
    EaseInOutQuint,
    /// Ease in expo
    EaseInExpo,
    /// Ease out expo
    EaseOutExpo,
    /// Ease in out expo
    EaseInOutExpo,
    /// Ease in circ
    EaseInCirc,
    /// Ease out circ
    EaseOutCirc,
    /// Ease in out circ
    EaseInOutCirc,
    /// Ease in back
    EaseInBack,
    /// Ease out back
    EaseOutBack,
    /// Ease in out back
    EaseInOutBack,
    /// Ease in elastic
    EaseInElastic,
    /// Ease out elastic
    EaseOutElastic,
    /// Ease in out elastic
    EaseInOutElastic,
    /// Ease in bounce
    EaseInBounce,
    /// Ease out bounce
    EaseOutBounce,
    /// Ease in out bounce
    EaseInOutBounce,
    #[cfg_attr(feature = "schemars", schemars(title = "CubicBezier"))]
    #[value(skip)]
    /// Custom Cubic Bezier function
    CubicBezier(f64, f64, f64, f64),
}

// Custom serde implementation
impl<'de> Deserialize<'de> for AnimationStyle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct AnimationStyleVisitor;

        impl<'de> serde::de::Visitor<'de> for AnimationStyleVisitor {
            type Value = AnimationStyle;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or an array of four f64 values")
            }

            // Handle string variants (e.g., "EaseInOutExpo")
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                value.parse().map_err(|_| E::unknown_variant(value, &[]))
            }

            // Handle CubicBezier array (e.g., [0.32, 0.72, 0.0, 1.0])
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let x1 = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let y1 = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                let x2 = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(2, &self))?;
                let y2 = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;

                // Ensure no extra elements
                if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::invalid_length(5, &self));
                }

                Ok(AnimationStyle::CubicBezier(x1, y1, x2, y2))
            }
        }

        deserializer.deserialize_any(AnimationStyleVisitor)
    }
}

impl Serialize for AnimationStyle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            // Serialize CubicBezier as an array
            AnimationStyle::CubicBezier(x1, y1, x2, y2) => {
                let mut seq = serializer.serialize_seq(Some(4))?;
                seq.serialize_element(x1)?;
                seq.serialize_element(y1)?;
                seq.serialize_element(x2)?;
                seq.serialize_element(y2)?;
                seq.end()
            }
            // Serialize all other variants as strings
            _ => serializer.serialize_str(&self.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_is_written_as_its_name_and_a_bezier_as_four_numbers() {
        assert_eq!(
            serde_json::to_string(&AnimationStyle::EaseInOutSine).unwrap(),
            r#""EaseInOutSine""#
        );
        assert_eq!(
            serde_json::to_string(&AnimationStyle::CubicBezier(0.32, 0.72, 0.0, 1.0)).unwrap(),
            "[0.32,0.72,0.0,1.0]"
        );
    }

    #[test]
    fn a_style_is_read_back_from_what_it_is_written_as() {
        for style in [
            AnimationStyle::Linear,
            AnimationStyle::EaseOutBounce,
            AnimationStyle::CubicBezier(0.1, 0.2, 0.3, 0.4),
        ] {
            let json = serde_json::to_string(&style).unwrap();
            assert_eq!(
                serde_json::from_str::<AnimationStyle>(&json).unwrap(),
                style
            );
        }
    }

    #[test]
    fn a_name_that_is_no_style_and_a_bezier_that_is_not_four_numbers_are_refused() {
        assert!(serde_json::from_str::<AnimationStyle>(r#""EaseInPlace""#).is_err());
        assert!(serde_json::from_str::<AnimationStyle>("[0.1, 0.2, 0.3]").is_err());
        assert!(serde_json::from_str::<AnimationStyle>("[0.1, 0.2, 0.3, 0.4, 0.5]").is_err());
    }
}
