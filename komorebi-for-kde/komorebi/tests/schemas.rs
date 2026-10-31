//! The JSON Schemas the daemon answers with: `StaticConfigSchema`,
//! `ApplicationSpecificConfigurationSchema`, `NotificationSchema` and `SocketSchema`, against an
//! in-memory desktop. `komorebic` prints the same schemas without the daemon; the daemon answers
//! on its socket for a client that has none. The schema of a place for a new window, which is
//! written by hand, is checked here too.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::PlacementTarget;
use komorebi::core::SocketMessage;
use komorebi::window_manager::WindowManager;
use serde_json::json;
use std::sync::Arc;

fn answer(message: SocketMessage) -> serde_json::Value {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = std::path::PathBuf::from(format!("/tmp/kms-{}.sock", std::process::id()));

    let mut reply = Vec::new();
    with_backend(fake, || {
        let mut wm = WindowManager::new(receiver, Some(socket.clone())).unwrap();
        wm.init().unwrap();
        wm.process_command(message, &mut reply)
    })
    .unwrap();
    let _ = std::fs::remove_file(&socket);

    serde_json::from_slice(&reply).expect("the answer is JSON")
}

#[test]
fn the_daemon_answers_with_the_schema_of_komorebi_json() {
    let schema = answer(SocketMessage::StaticConfigSchema);

    assert_eq!(schema["title"], "StaticConfig");
    assert!(schema["properties"]["monitors"].is_object());
    assert!(schema["properties"]["app_specific_configuration_path"].is_object());
}

#[test]
fn the_daemon_answers_with_the_schema_of_a_list_of_applications_files() {
    let schema = answer(SocketMessage::ApplicationSpecificConfigurationSchema);

    assert_eq!(schema["type"], "array");
    assert!(schema["$defs"]["AscApplicationRules"].is_object());
}

#[test]
fn the_daemon_answers_with_the_schemas_of_what_it_sends_and_of_what_it_takes() {
    assert!(answer(SocketMessage::NotificationSchema)["properties"]["state"].is_object());
    assert!(answer(SocketMessage::SocketSchema)["oneOf"].is_array());
}

#[test]
fn the_schema_of_komorebi_json_has_the_places_a_workspace_can_give_a_new_window() {
    let schema = answer(SocketMessage::StaticConfigSchema);

    let key = &schema["$defs"]["WorkspaceConfig"]["properties"]["initial_window_placement_rules"];
    assert!(key.is_object(), "a workspace has the key in the schema");
    assert!(schema["$defs"]["InitialWindowPlacementRules"]["anyOf"].is_array());
    assert!(schema["$defs"]["PlacementTarget"].is_object());
}

/// The schema derived for the enum is an object with a `Placement` or an `Index` key, which is not
/// what is read: a name, a number, or text that is a number.
#[test]
fn a_place_is_described_as_a_name_a_number_or_text_that_is_a_number() {
    let schema = serde_json::to_value(schemars::schema_for!(PlacementTarget)).unwrap();
    let forms = schema["anyOf"]
        .as_array()
        .expect("a place has several forms");
    assert_eq!(forms.len(), 3);

    // a name: every strategy of `WindowPlacement`, and each of them is read
    assert_eq!(forms[0]["$ref"], "#/$defs/WindowPlacement");
    let names: Vec<&str> = schema["$defs"]["WindowPlacement"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name["const"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 5);
    for name in &names {
        assert!(
            matches!(
                serde_json::from_value(json!(name)),
                Ok(PlacementTarget::Placement(_))
            ),
            "{name} is not read as a strategy"
        );
    }

    // a number
    assert_eq!(forms[1]["type"], "integer");
    assert_eq!(
        serde_json::from_value::<PlacementTarget>(json!(3)).unwrap(),
        PlacementTarget::Index(3)
    );

    // text that is a number: what the pattern accepts is what is read as a number, and a name is
    // not
    assert_eq!(forms[2]["type"], "string");
    let pattern = regex::Regex::new(forms[2]["pattern"].as_str().unwrap()).unwrap();
    assert!(pattern.is_match("3"));
    assert!(!pattern.is_match("Primary"));
    assert_eq!(
        serde_json::from_value::<PlacementTarget>(json!("3")).unwrap(),
        PlacementTarget::Index(3)
    );

    // and a place that is none of them is refused
    assert!(serde_json::from_value::<PlacementTarget>(json!("Nowhere")).is_err());
    assert!(!pattern.is_match("Nowhere"));
}
