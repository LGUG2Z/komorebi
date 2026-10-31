//! `SocketMessage`, the one message type of every komorebi socket.

use serde::Deserialize;
use serde::Serialize;

/// A state of the window manager as the JSON that `ApplyState` carries.
///
/// The daemon that handles `ApplyState` (Windows only) reads it into its own `State` type, which
/// does not live in this crate. On the wire it is exactly that state: the type is transparent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateBlob(pub serde_json::Value);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for StateBlob {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("State")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "The state of the window manager, as `komorebic state` prints it",
            "type": "object"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_blob_is_written_and_read_as_the_json_it_holds() {
        let text = r#"{"monitors":{"elements":[]}}"#;
        let blob: StateBlob = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::to_string(&blob).unwrap(), text);
    }
}
