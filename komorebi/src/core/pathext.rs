use std::env;
use std::path::Component;
use std::path::PathBuf;

pub trait PathExt {
    fn replace_env(&self) -> PathBuf;
}

impl PathExt for PathBuf {
    fn replace_env(&self) -> PathBuf {
        let mut result = PathBuf::new();

        for component in self.components() {
            match component {
                Component::Normal(segment) => {
                    // Check if it starts with `$` or `$Env:`
                    if let Some(stripped_segment) = segment.to_string_lossy().strip_prefix('$') {
                        let var_name = if let Some(env_name) = stripped_segment.strip_prefix("Env:")
                        {
                            // Extract the variable name after `$Env:`
                            env_name
                        } else if stripped_segment == "HOME" {
                            // Special case for `$HOME`
                            "USERPROFILE"
                        } else {
                            // Extract the variable name after `$`
                            stripped_segment
                        };

                        if let Ok(value) = env::var(var_name) {
                            result.push(&value); // Replace with the value
                        } else {
                            result.push(segment); // Keep as-is if variable is not found
                        }
                    } else {
                        result.push(segment); // Keep as-is if not an environment variable
                    }
                }
                _ => {
                    // Add other components (e.g., root, parent) as-is
                    result.push(component.as_os_str());
                }
            }
        }

        result
    }
}
