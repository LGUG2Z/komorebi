//! The JSON Schema of `komorebi.bar.json`, one per platform, as views of one union.
//!
//! The union is the Windows schema: every widget and option any bar reads, in Windows' spelling.
//! [`view_of`] turns it into the schema a platform checks in: it applies the platform's
//! [`changes`] and removes the definitions nothing refers to any more. The Windows view is the
//! union itself.
//!
//! The changes are idempotent (setting text that is already there, or removing what is not
//! there, does nothing), so a bar can print its own generated schema through its view.

pub mod changes;

use changes::Change;
use serde_json::Map;
use serde_json::Value;

/// A platform that has a bar
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaPlatform {
    Windows,
    MacOs,
}

/// The changes a platform's view applies, in order
#[must_use]
pub const fn changes_of(platform: SchemaPlatform) -> &'static [(&'static str, Change)] {
    match platform {
        SchemaPlatform::Windows => &[],
        SchemaPlatform::MacOs => changes::MACOS,
    }
}

/// The text a bar prints for `--schema`: pretty JSON followed by a newline, of the view of the
/// union of the bar configuration.
#[must_use]
pub fn view_text(union: Value, platform: SchemaPlatform) -> String {
    let mut text =
        serde_json::to_string_pretty(&view_of(union, platform)).expect("a schema is JSON");
    text.push('\n');
    text
}

/// The view of `schema` for `platform`.
#[must_use]
pub fn view_of(mut schema: Value, platform: SchemaPlatform) -> Value {
    for (pointer, change) in changes_of(platform) {
        apply(&mut schema, pointer, *change);
    }
    prune_unreachable(&mut schema);
    schema
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
        Change::RemoveTitled(title) => {
            if let Some(Value::Array(items)) = schema.pointer_mut(pointer) {
                items.retain(|item| item.get("title").and_then(Value::as_str) != Some(title));
            }
        }
    }
}

/// Keep the definitions that the schema refers to, directly or through another definition.
fn prune_unreachable(schema: &mut Value) {
    let mut reachable = Vec::new();
    collect_refs(schema, &mut reachable);
    let Some(defs) = schema.get("$defs").and_then(Value::as_object).cloned() else {
        return;
    };
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
        .filter(|(name, _)| reachable.contains(name))
        .collect();
    schema["$defs"] = Value::Object(kept);
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

/// The text `--schema` prints for the bar configuration type `T`, as `platform` checks it in.
#[cfg(feature = "schemars")]
#[must_use]
pub fn generated_text<T: schemars::JsonSchema>(platform: SchemaPlatform) -> String {
    let union = serde_json::to_value(schemars::schema_for!(T)).expect("a schema is JSON");
    view_text(union, platform)
}
