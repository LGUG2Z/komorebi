use crate::monitor::MonitorInfo;
use color_eyre::eyre;
use color_eyre::eyre::bail;
use quickxml_to_serde::Config;
use quickxml_to_serde::xml_string_to_json;
use std::process::Command;

use serde_json::Value;
pub struct IoReg;

impl IoReg {
    #[tracing::instrument]
    pub fn query_monitors() -> eyre::Result<Vec<MonitorInfo>> {
        let output = Command::new("ioreg")
            .arg("-a")
            .arg("-c")
            .arg("IOMobileFramebufferShim")
            .output()?;

        if !output.status.success() {
            bail!(
                "ioreg command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let xml_output = String::from_utf8(output.stdout)?;
        let value = xml_string_to_json(xml_output, &Config::new_with_defaults())?;
        let mut results = vec![];
        find_display_entries(&value, &mut results);

        Ok(results)
    }
}

fn find_display_entries(value: &Value, results: &mut Vec<MonitorInfo>) {
    match value {
        Value::Object(obj) => {
            if let (
                Some(Value::Array(keys)),
                Some(Value::Array(strings)),
                Some(Value::Array(integers)),
            ) = (obj.get("key"), obj.get("string"), obj.get("integer"))
            {
                let has_product_name = keys
                    .iter()
                    .any(|k| k.as_str().map(|s| s == "ProductName").unwrap_or(false));

                if has_product_name
                    && strings.len() >= 3
                    && integers.len() >= 5
                    && let (Some(alpha_serial), Some(manuf_id), Some(product_name)) = (
                        strings[0].as_str(),
                        strings[1].as_str(),
                        strings[2].as_str(),
                    )
                {
                    results.push(MonitorInfo {
                        alphanumeric_serial_number: alpha_serial.to_string(),
                        manufacturer_id: manuf_id.to_string(),
                        product_name: product_name.to_string(),
                        legacy_manufacturer_id: integers[0].as_u64().unwrap_or(0).to_string(),
                        product_id: integers[1].as_u64().unwrap_or(0).to_string(),
                        serial_number: integers[2].as_u64().unwrap_or(0) as u32,
                        week_of_manufacture: integers[3].as_u64().unwrap_or(0).to_string(),
                        year_of_manufacture: integers[4].as_u64().unwrap_or(0).to_string(),
                    });
                }
            }

            for v in obj.values() {
                find_display_entries(v, results);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                find_display_entries(item, results);
            }
        }
        _ => {}
    }
}
