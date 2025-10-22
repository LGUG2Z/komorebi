use clap::ValueEnum;

use serde::Deserialize;
use serde::Serialize;
use serde::ser::SerializeSeq;
use strum::Display;
use strum::EnumString;

#[derive(Copy, Clone, Debug, Display, EnumString, ValueEnum, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum AnimationStyle {
    Linear,
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseInCubic,
    EaseInOutCubic,
    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,
    EaseInQuint,
    EaseOutQuint,
    EaseInOutQuint,
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,
    EaseInCirc,
    EaseOutCirc,
    EaseInOutCirc,
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    EaseInElastic,
    EaseOutElastic,
    EaseInOutElastic,
    EaseInBounce,
    EaseOutBounce,
    EaseInOutBounce,
    #[value(skip)]
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
