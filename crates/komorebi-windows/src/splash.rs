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
use std::path::PathBuf;
use std::process::Command;

pub fn mdm_enrollment() -> eyre::Result<(bool, Option<String>)> {
    let mut command = Command::new("dsregcmd");
    command.args(["/status"]);
    let stdout = command.output()?.stdout;
    let output = std::str::from_utf8(&stdout)?;
    if !output.contains("WorkspaceTenantName") {
        return Ok((false, None));
    }

    let mut tenant = None;

    for line in output.lines() {
        if line.contains("WorkspaceTenantName") {
            let line = line.trim().to_string();
            tenant = Some(
                line.trim_start_matches("WorkspaceTenantName : ")
                    .to_string(),
            )
        }
    }

    Ok((true, tenant))
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
        .get("https://kw-icul.lgug2z.com")
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
