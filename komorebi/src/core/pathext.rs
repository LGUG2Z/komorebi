use std::env;
use std::ffi::OsStr;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

/// Path extension trait
pub trait PathExt {
    /// Resolve environment variables components in a path.
    ///
    /// Resolves the follwing formats:
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
                        if let Some(value) = env::var_os(var) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_env_vars() {
        // helper functions
        fn expected<P: AsRef<Path>>(p: P) -> PathBuf {
            // Ensure that the path is using the correct path separator for the OS.
            p.as_ref().components().collect::<PathBuf>()
        }

        fn resolve<P: AsRef<Path>>(p: P) -> PathBuf {
            p.replace_env()
        }

        // Set a variable for testing
        std::env::set_var("VAR", "VALUE");

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
        std::env::set_var("USERPROFILE", "C:\\Users\\user");

        // ~ and $HOME should be replaced with $Env:USERPROFILE
        assert_eq!(resolve("~"), expected("C:\\Users\\user"));
        assert_eq!(resolve("$HOME"), expected("C:\\Users\\user"));
    }
}
