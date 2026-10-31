//! How the animation settings of `komorebi.json` are scoped.

use std::collections::HashMap;

use komorebi_protocol::animation::prefix::AnimationPrefix;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Animation configuration
///
/// This can be either global:
/// ```json
/// {
///     "enabled": true,
///     "style": "EaseInSine",
///     "fps": 60,
///     "duration": 250
/// }
/// ```
///
/// Or scoped by an animation kind prefix:
/// ```json
/// {
///     "movement": {
///         "enabled": true,
///         "style": "EaseInSine",
///         "fps": 60,
///         "duration": 250
///     }
/// }
/// ```
pub enum PerAnimationPrefixConfig<T> {
    /// Animation configuration prefixed for a specific animation kind
    Prefix(HashMap<AnimationPrefix, T>),
    /// Animation configuration for all animation kinds
    Global(T),
}
