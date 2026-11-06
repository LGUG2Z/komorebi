//! Generation of the JSON Schema files that are checked in next to the sources.
//!
//! The data types describe every key of `komorebi.json` that any platform reads. A key (or a
//! field of a nested type) that only some platforms read carries `x-komorebi-platforms`, a list
//! of platform names, in its schema. The schema a platform checks in is a *view* of that union:
//! [`view`] drops what the platform does not read, strips the `x-komorebi-` markers and
//! removes the definitions nothing refers to any more.

use crate::changes;
use crate::changes::Change;
pub use komorebi_protocol::platforms::Platform;
use schemars::JsonSchema;
use serde_json::Map;
use serde_json::Value;

/// The key that lists the platforms that read a property.
pub const PLATFORMS: &str = "x-komorebi-platforms";

/// The name a platform has in `x-komorebi-platforms`.
#[must_use]
pub const fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "windows",
        Platform::MacOs => "macos",
        Platform::Kde => "kde",
    }
}

/// The text `komorebic` prints for the schema of `T`: pretty JSON followed by a newline. This is
/// the union over all platforms.
#[must_use]
pub fn pretty<T: JsonSchema>() -> String {
    to_text(&union::<T>())
}

/// The schema of `T` as one JSON value, the union over all platforms.
#[must_use]
pub fn union<T: JsonSchema>() -> Value {
    let schema = schemars::schema_for!(T);
    serde_json::to_value(schema).expect("a schema is JSON")
}

/// The schema text that is checked in for `platform`: the view of the union of `T`.
#[must_use]
pub fn view<T: JsonSchema>(platform: Platform) -> String {
    to_text(&view_of(union::<T>(), platform))
}

fn to_text(schema: &Value) -> String {
    let mut text = serde_json::to_string_pretty(schema).expect("a schema is JSON");
    text.push('\n');
    text
}

/// Prune `schema` to what `platform` reads.
#[must_use]
pub fn view_of(mut schema: Value, platform: Platform) -> Value {
    let name = platform_name(platform);
    prune(&mut schema, name);
    for (def, pointer, text) in platform_text(platform) {
        if let Some(target) = schema.pointer_mut(&format!("/$defs/{def}{pointer}")) {
            *target = Value::String((*text).to_string());
        }
    }
    for (pointer, change) in changes::for_platform(platform) {
        apply(&mut schema, pointer, *change);
    }
    let mut reachable = Vec::new();
    collect_refs(&schema, &mut reachable);
    if let Some(defs) = schema.get("$defs").and_then(Value::as_object).cloned() {
        // a definition can refer to others: follow until nothing new is found
        let mut next = 0;
        while next < reachable.len() {
            if let Some(def) = defs.get(&reachable[next]) {
                let mut found = Vec::new();
                collect_refs(def, &mut found);
                for name in found {
                    if !reachable.contains(&name) {
                        reachable.push(name);
                    }
                }
            }
            next += 1;
        }
        let kept: Map<String, Value> = defs
            .into_iter()
            .filter(|(n, _)| reachable.contains(n))
            .collect();
        schema["$defs"] = Value::Object(kept);
    }
    schema
}

/// Remove the properties `platform` does not read, everywhere, and the markers of the rest.
fn prune(value: &mut Value, platform: &str) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get_mut("properties") {
                let gone: Vec<String> = properties
                    .iter()
                    .filter(|(_, property)| !reads(property, platform))
                    .map(|(key, _)| key.clone())
                    .collect();
                properties.retain(|key, _| !gone.contains(key));
                if let Some(Value::Array(required)) = map.get_mut("required") {
                    required.retain(|key| !gone.iter().any(|g| key.as_str() == Some(g)));
                }
            }
            map.remove(PLATFORMS);
            for child in map.values_mut() {
                prune(child, platform);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| prune(item, platform)),
        _ => {}
    }
}

/// A property without the marker is read by every platform.
fn reads(property: &Value, platform: &str) -> bool {
    match property.get(PLATFORMS) {
        Some(Value::Array(platforms)) => platforms.iter().any(|p| p.as_str() == Some(platform)),
        _ => true,
    }
}

fn collect_refs(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(target)) = map.get("$ref")
                && let Some(name) = target.strip_prefix("#/$defs/")
                && !found.iter().any(|n| n == name)
            {
                found.push(name.to_string());
            }
            for (key, child) in map {
                if key != "$defs" {
                    collect_refs(child, found);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_refs(item, found)),
        _ => {}
    }
}

const WINDOWS_PATH: &str = "A file system path. Environment variables like %VAR%, $Env:VAR, or $VAR are automatically resolved.";
const UNIX_PATH: &str =
    "A file system path. Environment variables like $VAR are automatically resolved.";
const FFM_KDE: &str = "Focus follows mouse implementation\n\nThe two exist on Windows. On KDE the compositor does it, so both ask KWin for its\n\"focus follows mouse\" focus policy.";

/// Descriptions of definitions that live in `komorebi-protocol` and read differently on each
/// platform: (definition, JSON pointer inside it, text). The protocol crate picks the text by
/// the platform it is compiled for; a view is the same on every host, so it says them here.
fn platform_text(platform: Platform) -> &'static [(&'static str, &'static str, &'static str)] {
    match platform {
        Platform::Windows => &[
            ("PathBuf", "/description", WINDOWS_PATH),
            ("ApplicationIdentifier", "/oneOf/1/description", "Class"),
            (
                "FocusFollowsMouseImplementation",
                "/description",
                "Focus follows mouse implementation",
            ),
        ],
        Platform::Kde => &[
            ("PathBuf", "/description", UNIX_PATH),
            (
                "ApplicationIdentifier",
                "/oneOf/1/description",
                "Role or subrole",
            ),
            ("FocusFollowsMouseImplementation", "/description", FFM_KDE),
        ],
        Platform::MacOs => &[
            ("PathBuf", "/description", UNIX_PATH),
            (
                "ApplicationIdentifier",
                "/oneOf/1/description",
                "Accessibility subrole or role, such as AXStandardWindow",
            ),
        ],
    }
}

fn apply(schema: &mut Value, pointer: &str, change: Change) {
    match change {
        Change::Set(json) => {
            if let Some(target) = schema.pointer_mut(pointer) {
                *target = serde_json::from_str(json).expect("a change is JSON");
            }
        }
        Change::Remove => {
            if let Some((parent, key)) = pointer.rsplit_once('/')
                && let Some(Value::Object(map)) = schema.pointer_mut(parent)
            {
                map.retain(|k, _| k != key);
            }
        }
    }
}
