use crate::DATA_DIR;
use crate::License;
use crate::PUBLIC_KEY;
use base64::Engine;
use base64::engine::general_purpose;
use chrono::Duration;
use chrono::TimeZone;
use chrono::Utc;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use ed25519_dalek::Verifier;
use ed25519_dalek::VerifyingKey;
use objc2::MainThreadMarker;
use objc2::msg_send;
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSAlert;
use objc2_app_kit::NSAlertStyle;
use objc2_app_kit::NSApplication;
use objc2_app_kit::NSFloatingWindowLevel;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;
use objc2_foundation::NSURL;
use std::path::PathBuf;
use std::process::Command;

pub fn mdm_enrollment() -> eyre::Result<(bool, Option<String>)> {
    let mut command = Command::new("/usr/bin/profiles");
    command.args(["status", "-type", "enrollment"]);
    let stdout = command.output()?.stdout;
    let output = std::str::from_utf8(&stdout)?;
    if output.contains("MDM enrollment: No") {
        return Ok((false, None));
    }

    let mut server = None;

    for line in output.lines() {
        if line.starts_with("MDM server") {
            server = Some(line.trim_start_matches("MDM server: ").to_string())
        }
    }

    Ok((true, server))
}

fn is_valid_payload(raw: &str, fresh: bool) -> eyre::Result<bool> {
    let mut validation_successful = false;

    let payload = serde_json::from_str::<License>(raw)?;

    let signature = ed25519_dalek::Signature::from_slice(
        general_purpose::STANDARD
            .decode(&payload.signature)?
            .as_slice(),
    )?;

    let mut value: serde_json::Value = serde_json::from_str(raw)?;
    if let serde_json::Value::Object(ref mut map) = value {
        map.remove("signature");
    }

    let message_to_verify = serde_json::to_string(&value)?;
    let verifying_key = VerifyingKey::from_bytes(&PUBLIC_KEY)?;

    if verifying_key
        .verify(message_to_verify.as_bytes(), &signature)
        .is_ok()
    {
        if fresh {
            let timestamp = Utc
                .timestamp_opt(payload.timestamp, 0)
                .single()
                .ok_or_eyre("invalid timestamp")?;

            let valid_duration = Utc::now() - Duration::minutes(5);

            if timestamp <= valid_duration {
                tracing::debug!("individual commercial use license verification payload was stale");
                return Ok(true);
            }
        }

        if payload.has_valid_subscription
            && let Some(current_end_period) = payload.current_end_period
        {
            let subscription_valid_until = Utc
                .timestamp_opt(current_end_period, 0)
                .single()
                .ok_or_eyre("invalid timestamp")?;

            if Utc::now() <= subscription_valid_until {
                tracing::debug!(
                    "individual commercial use license verification - subscription valid until: {subscription_valid_until}",
                );

                validation_successful = true;
            }
        }
    }

    Ok(validation_successful)
}

pub enum ValidationFeedback {
    Successful(PathBuf),
    Unsuccessful(String),
    NoEmail,
    NoConnectivity,
}

impl From<ValidationFeedback> for bool {
    fn from(value: ValidationFeedback) -> Self {
        match value {
            ValidationFeedback::Successful(_) => false,
            ValidationFeedback::Unsuccessful(_)
            | ValidationFeedback::NoEmail
            | ValidationFeedback::NoConnectivity => true,
        }
    }
}

pub fn should() -> eyre::Result<ValidationFeedback> {
    let icul_validation = DATA_DIR.join("icul.validation");
    if icul_validation.exists() {
        tracing::debug!("found local individual commercial use license validation payload");
        let raw_payload = std::fs::read_to_string(&icul_validation)?;
        if is_valid_payload(&raw_payload, false)? {
            return Ok(ValidationFeedback::Successful(icul_validation));
        } else {
            std::fs::remove_file(&icul_validation)?;
        }
    }

    let icul = DATA_DIR.join("icul");
    if !icul.exists() {
        return Ok(ValidationFeedback::NoEmail);
    }

    let email = std::fs::read_to_string(icul)?;
    tracing::debug!("found individual commercial use license email: {}", email);

    let client = reqwest::blocking::Client::new();
    let response = match client
        .get("https://km-icul.lgug2z.com")
        .query(&[("email", email.trim())])
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            tracing::error!("{error}");
            return Ok(ValidationFeedback::NoConnectivity);
        }
    };

    let raw_payload = response.text()?;
    if is_valid_payload(&raw_payload, true)? {
        std::fs::write(&icul_validation, &raw_payload)?;
        Ok(ValidationFeedback::Successful(icul_validation))
    } else {
        Ok(ValidationFeedback::Unsuccessful(raw_payload))
    }
}

pub fn show(server: Option<String>) -> eyre::Result<()> {
    autoreleasepool(|_| {
        let mtm = MainThreadMarker::new().unwrap();

        let app = NSApplication::sharedApplication(mtm);
        let alert = NSAlert::new(mtm);
        alert.setAlertStyle(NSAlertStyle::Critical);
        alert.setMessageText(&NSString::from_str("MDM Enrollment Detected"));
        let informative_text = match server {
                    None => {
                        "It looks like you are using a corporate device enrolled in mobile device management\n\n\
                         The Komorebi License does not permit any kind of commercial use\n\n\
                         A dedicated Individual Commercial Use License is available if you wish to use this software at work\n\n\
                         You are strongly encouraged to make your employer pay for your license, either directly or via reimbursement\n\n\
                         To remove this popup in the future, run \"komorebic license <email>\" using the email address associated with your license".to_string()
                    }
                    Some(server) => {
                        format!(
                            "It looks like you are using a corporate device enrolled in mobile device management ({server})\n\n\
                             The Komorebi License does not permit any kind of commercial use\n\n\
                             A dedicated Individual Commercial Use License is available if you wish to use this software at work\n\n\
                             You are strongly encouraged to make your employer pay for your license, either directly or via reimbursement\n\n\
                             To remove this popup in the future you can run \"komorebic license <email>\" using the email address associated with your license"
                        )
                    }
                };

        alert.setInformativeText(&NSString::from_str(&informative_text));
        alert.addButtonWithTitle(&NSString::from_str("Purchase License"));
        alert.addButtonWithTitle(&NSString::from_str("Dismiss"));

        let window = alert.window();

        window.center();
        window.setLevel(NSFloatingWindowLevel);
        window.setHidesOnDeactivate(true);
        window.makeKeyAndOrderFront(None);
        window.orderFrontRegardless();

        app.activate();

        let response = alert.runModal();

        if response == 1000 {
            let _ = open_url_in_browser("https://lgug2z.com/software/komorebi");
        }
    });

    Ok(())
}

fn open_url_in_browser(url_string: &str) -> eyre::Result<()> {
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let url_ns_string = NSString::from_str(url_string);
        let url = NSURL::URLWithString(&url_ns_string).ok_or_eyre("failed to create NSURL")?;

        let success: bool = msg_send![&*workspace, openURL: &*url];

        if !success {
            tracing::error!("failed to open URL: {}", url_string);
        }

        Ok(())
    }
}
