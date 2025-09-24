use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Path extension trait
pub trait PathExt {
    /// Resolve environment variable components in a path.
    ///
    /// Resolves the following formats:
    /// - CMD: `%variable%`
    /// - PowerShell: `$Env:variable`
    /// - Bash: `$variable`.
    fn replace_env(&self) -> PathBuf;
}

/// Blanket implementation for all types that can be converted to a `Path`.
impl<P: AsRef<Path>> PathExt for P {
    fn replace_env(&self) -> PathBuf {
        let mut out = PathBuf::new();

        for c in self.as_ref().components() {
            match c {
                Component::Normal(mut c) => {
                    // Special case for ~ and $HOME, replace with $Env:USERPROFILE
                    if c == OsStr::new("~") || c.eq_ignore_ascii_case("$HOME") {
                        c = OsStr::new("$Env:USERPROFILE");
                    }

                    let bytes = c.as_encoded_bytes();

                    // %LOCALAPPDATA%
                    let var = if bytes[0] == b'%' && bytes[bytes.len() - 1] == b'%' {
                        Some(&bytes[1..bytes.len() - 1])
                    } else {
                        // prefix length is 5 for $Env: and 1 for $
                        // so we take the minimum of 5 and the length of the bytes
                        let prefix = &bytes[..5.min(bytes.len())];
                        let prefix = unsafe { OsStr::from_encoded_bytes_unchecked(prefix) };

                        // $Env:LOCALAPPDATA
                        if prefix.eq_ignore_ascii_case("$Env:") {
                            Some(&bytes[5..])
                        } else if bytes[0] == b'$' {
                            // $LOCALAPPDATA
                            Some(&bytes[1..])
                        } else {
                            // not a variable
                            None
                        }
                    };

                    // if component is a variable, get the value from the environment
                    if let Some(var) = var {
                        let var = unsafe { OsStr::from_encoded_bytes_unchecked(var) };
                        if let Some(value) = std::env::var_os(var) {
                            out.push(value);
                            continue;
                        }
                    }

                    // if not a variable, or a value couldn't be obtained from environemnt
                    // then push the component as is
                    out.push(c);
                }

                // other components are pushed as is
                _ => out.push(c),
            }
        }

        out
    }
}

/// Replace environment variables in a path. This is a wrapper around
/// [`PathExt::replace_env`] to be used in Clap arguments parsing.
pub fn replace_env_in_path(input: &str) -> Result<PathBuf, std::convert::Infallible> {
    Ok(input.replace_env())
}

/// A wrapper around [`PathBuf`] that has a custom [Deserialize] implementation
/// that uses [`PathExt::replace_env`] to resolve environment variables
#[derive(Clone, Debug)]
pub struct ResolvedPathBuf(PathBuf);

impl ResolvedPathBuf {
    /// Create a new [`ResolvedPathBuf`] from a [`PathBuf`]
    pub fn new(path: PathBuf) -> Self {
        Self(path.replace_env())
    }
}

impl From<ResolvedPathBuf> for PathBuf {
    fn from(path: ResolvedPathBuf) -> Self {
        path.0
    }
}

impl serde_with::SerializeAs<PathBuf> for ResolvedPathBuf {
    fn serialize_as<S>(path: &PathBuf, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        path.serialize(serializer)
    }
}

impl<'de> serde_with::DeserializeAs<'de, PathBuf> for ResolvedPathBuf {
    fn deserialize_as<D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        Ok(path.replace_env())
    }
}

#[cfg(feature = "schemars")]
impl serde_with::schemars_0_8::JsonSchemaAs<PathBuf> for ResolvedPathBuf {
    fn schema_name() -> String {
        "PathBuf".to_owned()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::schema::Schema {
        <PathBuf as schemars::JsonSchema>::json_schema(generator)
    }
}

/// Custom deserializer for [`Option<HashMap<usize, PathBuf>>`] that uses
/// [`PathExt::replace_env`] to resolve environment variables in the paths.
///
/// This is used in `WorkspaceConfig` struct because we can't use
/// #[serde_with::serde_as] as it doesn't handle [`Option<HashMap<usize, ResolvedPathBuf>>`]
/// quite well and generated compiler errors that can't be fixed because of Rust's orphan rule.
pub fn resolve_option_hashmap_usize_path<'de, D>(
    deserializer: D,
) -> Result<Option<HashMap<usize, PathBuf>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let map = Option::<HashMap<usize, PathBuf>>::deserialize(deserializer)?;
    Ok(map.map(|map| map.into_iter().map(|(k, v)| (k, v.replace_env())).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // helper functions
    fn expected<P: AsRef<Path>>(p: P) -> PathBuf {
        // Ensure that the path is using the correct path separator for the OS.
        p.as_ref().components().collect::<PathBuf>()
    }

    fn resolve<P: AsRef<Path>>(p: P) -> PathBuf {
        p.replace_env()
    }

    #[test]
    fn resolves_env_vars() {
        // Set a variable for testing
        unsafe {
            std::env::set_var("VAR", "VALUE");
        }

        // %VAR% format
        assert_eq!(resolve("/path/%VAR%/d"), expected("/path/VALUE/d"));
        // $env:VAR format
        assert_eq!(resolve("/path/$env:VAR/d"), expected("/path/VALUE/d"));
        // $VAR format
        assert_eq!(resolve("/path/$VAR/d"), expected("/path/VALUE/d"));

        // non-existent variable
        assert_eq!(resolve("/path/%ASD%/to/d"), expected("/path/%ASD%/to/d"));
        assert_eq!(
            resolve("/path/$env:ASD/to/d"),
            expected("/path/$env:ASD/to/d")
        );
        assert_eq!(resolve("/path/$ASD/to/d"), expected("/path/$ASD/to/d"));

        // Set a $env:USERPROFILE variable for testing
        unsafe {
            std::env::set_var("USERPROFILE", "C:\\Users\\user");
        }

        // ~ and $HOME should be replaced with $Env:USERPROFILE
        assert_eq!(resolve("~"), expected("C:\\Users\\user"));
        assert_eq!(resolve("$HOME"), expected("C:\\Users\\user"));
    }
}
