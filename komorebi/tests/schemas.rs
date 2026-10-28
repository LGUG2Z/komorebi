//! The JSON Schemas the daemon answers with: `StaticConfigSchema`,
//! `ApplicationSpecificConfigurationSchema`, `NotificationSchema` and `SocketSchema`, against an
//! in-memory desktop. `komorebic` prints the same schemas without the daemon; the daemon answers
//! on its socket for a client that has none.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::SocketMessage;
use komorebi::window_manager::WindowManager;
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
