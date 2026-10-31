//! Which platform reads which key of `komorebi.json`.
//!
//! The table is not written a second time: it is read from the `x-komorebi-platforms` markers
//! of the one `StaticConfig`, so a key cannot be added without saying who reads it.

use komorebi_protocol::platforms::Platform;
use serde_json::Value;

use crate::schema::PLATFORMS;
use crate::schema::platform_name;
use crate::schema::union;
use crate::static_config::StaticConfig;

/// Whether `platform` reads the top-level `key` of a configuration. A key the type does not
/// know is read by nobody (it is ignored everywhere).
#[must_use]
pub fn reads(platform: Platform, key: &str) -> bool {
    let schema = union::<StaticConfig>();
    match schema["properties"].get(key) {
        None => false,
        Some(property) => match property.get(PLATFORMS) {
            Some(Value::Array(platforms)) => platforms
                .iter()
                .any(|p| p.as_str() == Some(platform_name(platform))),
            _ => true,
        },
    }
}

/// The top-level keys of the configuration text `raw` that are keys of `komorebi.json` but that
/// `platform` does not read, in file order: what a daemon says at load time instead of silently
/// dropping them. Keys that are not known at all are not reported.
///
/// # Errors
///
/// The text is not JSON.
pub fn unsupported_keys(raw: &str, platform: Platform) -> color_eyre::Result<Vec<String>> {
    let value: Value = serde_json::from_str(raw)?;
    let schema = union::<StaticConfig>();
    let known = schema["properties"].as_object();
    Ok(value
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .filter(|key| known.is_some_and(|k| k.contains_key(*key)) && !reads(platform, key))
        .cloned()
        .collect())
}
