//! Generation of the JSON Schema files that are checked in next to the sources.

use schemars::JsonSchema;

/// The text `komorebic` prints for the schema of `T` (and that is checked in): pretty
/// JSON followed by a newline.
#[must_use]
pub fn pretty<T: JsonSchema>() -> String {
    let schema = schemars::schema_for!(T);
    let mut text = serde_json::to_string_pretty(&schema).expect("a schema is JSON");
    text.push('\n');
    text
}
