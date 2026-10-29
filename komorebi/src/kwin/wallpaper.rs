//! The wallpaper of a monitor, set through the Plasma shell.
//!
//! Plasma keeps one wallpaper for each screen, not for each virtual desktop, so komorebi sets a
//! monitor's wallpaper whenever one of its workspaces comes into view: the workspace's wallpaper,
//! else the monitor's, when there is one. The Plasma shell, a separate process, sets it over D-Bus
//! with `org.kde.PlasmaShell.setWallpaper`: the call `plasma-apply-wallpaperimage` makes, but for
//! one screen instead of all of them. [`apply`] finds the shell's screen for an output, asks what
//! it shows, and changes it only when it shows something else.
//!
//! The shell may not be running (a headless session, a session that has not finished starting, or
//! another shell). [`apply`] then fails, and the KWin backend tries again for about half a minute
//! before it leaves the wallpaper alone and logs that once.
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof |
//! |---|---|---|
//! | The name of an output cannot end the string it is in, in the script sent to the shell | [`screen_script`] writes it as a JSON string | `kwin::wallpaper::tests::the_name_of_an_output_cannot_end_the_string_it_is_in` |
//! | Any path is a URL the shell can read | [`file_url`] percent-encodes every byte but ASCII letters, digits and `-._~/` | `kwin::wallpaper::tests::a_path_becomes_a_url_the_shell_can_read` |
//! | A wallpaper the screen already shows is not set again, and one the user changed by hand is set back | [`apply`] compares with what the shell answers | the second half: the `vm-wallpaper` VM suite; the first: no test |

#![deny(missing_docs)]

use color_eyre::eyre;
use std::collections::HashMap;
use std::path::Path;
use zbus::Connection;
use zbus::OwnedVariant;
use zbus::Variant;
use zbus::proxy::Proxy;

/// The Plasma shell's name on the session bus.
const SHELL: &str = "org.kde.plasmashell";
/// The object of the shell that sets wallpapers.
const PATH: &str = "/PlasmaShell";
/// The interface of [`PATH`].
const INTERFACE: &str = "org.kde.PlasmaShell";

/// The wallpaper plugin that shows a picture.
const PLUGIN: &str = "org.kde.image";

/// Qt's `Image.PreserveAspectCrop`: the picture fills the screen and is cropped where it does not
/// fit, as komorebi's wallpapers do on Windows (`DWPOS_FILL`).
const FILL_MODE: i32 = 2;

/// Returns the `file://` URL of a path, which is how Plasma keeps the picture.
///
/// Every byte of the path other than an ASCII letter or digit, `-`, `.`, `_`, `~` and `/` is
/// percent-encoded, so `/a/b c.png` is `file:///a/b%20c.png`. Covered by
/// `kwin::wallpaper::tests::a_path_becomes_a_url_the_shell_can_read`.
pub fn file_url(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;

    let mut url = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                url.push(byte as char);
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

/// Returns the Plasma shell script, for `evaluateScript`, that prints the number the shell gives an
/// output (`-1` for none).
///
/// The shell numbers its screens itself, and identifies them by the connector's name, which is the
/// name KWin gives the output.
fn screen_script(output: &str) -> String {
    let name = serde_json::to_string(output).unwrap_or_else(|_| "\"\"".into());
    format!("print(screenForConnector({name}))")
}

/// Returns true if and only if the shell's answer about a screen's wallpaper is the picture at
/// `url`, shown by [`PLUGIN`] with [`FILL_MODE`].
fn shows(current: &HashMap<String, OwnedVariant>, url: &str) -> bool {
    let text = |key: &str| {
        current
            .get(key)
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
    };
    let fill = current
        .get("FillMode")
        .and_then(|v| i32::try_from(v.try_clone().ok()?).ok());

    text("wallpaperPlugin").as_deref() == Some(PLUGIN)
        && text("Image").as_deref() == Some(url)
        && fill == Some(FILL_MODE)
}

/// Makes `path` the wallpaper of the output with this name, unless it already is, and returns true
/// if and only if it changed the wallpaper.
///
/// # Errors
///
/// Returns an error when a call to the shell fails (it is not on the bus, for one), when its answer
/// for the screen of the output is not a number, and when it has no screen for the output.
pub async fn apply(connection: &Connection, output: &str, path: &Path) -> eyre::Result<bool> {
    let proxy = Proxy::new(connection, SHELL, PATH, INTERFACE).await?;

    let answer: String = proxy
        .call("evaluateScript", &(screen_script(output),))
        .await?;
    let screen: i64 = answer.trim().parse().map_err(|_| {
        eyre::Report::msg(format!(
            "the Plasma shell answered {answer:?} for the screen of {output}"
        ))
    })?;
    let Ok(screen) = u32::try_from(screen) else {
        eyre::bail!("the Plasma shell has no screen for {output}");
    };

    let url = file_url(path);
    let current: HashMap<String, OwnedVariant> = proxy.call("wallpaper", &(screen,)).await?;
    if shows(&current, &url) {
        return Ok(false);
    }

    let mut parameters: HashMap<&str, Variant<'_>> = HashMap::new();
    parameters.insert("Image", Variant::from(url));
    parameters.insert("FillMode", Variant::from(FILL_MODE));
    proxy
        .call_method("setWallpaper", &(PLUGIN, parameters, screen))
        .await?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_a_url_the_shell_can_read() {
        assert_eq!(file_url(Path::new("/tmp/a.png")), "file:///tmp/a.png");
        assert_eq!(
            file_url(Path::new("/home/me/My Pictures/sea & sky.png")),
            "file:///home/me/My%20Pictures/sea%20%26%20sky.png"
        );
        // not ASCII: each byte of the UTF-8 is escaped
        assert_eq!(file_url(Path::new("/tmp/é.png")), "file:///tmp/%C3%A9.png");
    }

    #[test]
    fn the_name_of_an_output_cannot_end_the_string_it_is_in() {
        assert_eq!(
            screen_script("DP-1"),
            r#"print(screenForConnector("DP-1"))"#
        );
        let hostile = screen_script(r#"x"); evil("#);
        assert_eq!(hostile, r#"print(screenForConnector("x\"); evil("))"#);
    }
}
