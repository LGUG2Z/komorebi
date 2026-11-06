//! What the test window is told, on every platform: its command line, and the lines on its standard
//! input. Nothing here knows a window system, so it is tested anywhere.

use std::io::Write;

/// What the window is started with.
#[derive(Debug, PartialEq)]
pub struct Options {
    pub title: String,
    /// The size of the window, in pixels (points on macOS).
    pub size: (i32, i32),
    /// The window class on Windows; on macOS, which has none, the accessibility subrole of the
    /// window, which is what the daemon takes for one; no other platform has one, and ignores it.
    pub class: Option<String>,
    /// `0xRRGGBB`.
    pub color: Option<u32>,
    pub min_size: Option<(i32, i32)>,
    /// Not resizable.
    pub fixed: bool,
    /// A title the window gives itself, and how many seconds after it opens.
    pub rename: Option<(String, f64)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            title: String::from("komorebi-testwin"),
            size: (400, 300),
            class: None,
            color: None,
            min_size: None,
            fixed: false,
            rename: None,
        }
    }
}

/// `RRGGBB`, with or without a `#`, as `0xRRGGBB`.
pub fn parse_color(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim_start_matches('#'), 16)
        .ok()
        .filter(|rgb| *rgb <= 0xff_ffff)
}

pub fn parse_size(s: &str) -> Option<(i32, i32)> {
    let (w, h) = s.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// The options of a command line (without the program name). What is not understood is said on
/// standard error and left out, as a window that opens is worth more to a scenario than one that
/// refuses.
pub fn parse(args: impl IntoIterator<Item = String>) -> Options {
    let mut options = Options::default();
    let mut rename_to = None;
    let mut after = 0.0;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--title" => options.title = args.next().unwrap_or(options.title),
            "--class" => options.class = args.next(),
            "--color" => options.color = args.next().as_deref().and_then(parse_color),
            "--size" => {
                options.size = args
                    .next()
                    .as_deref()
                    .and_then(parse_size)
                    .unwrap_or(options.size)
            }
            "--min-size" => options.min_size = args.next().as_deref().and_then(parse_size),
            "--fixed" => options.fixed = true,
            "--rename-to" => rename_to = args.next(),
            "--after" => after = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            other => eprintln!("komorebi-testwin: ignoring {other}"),
        }
    }
    options.rename = rename_to.map(|title| (title, after));
    options
}

/// One line on standard input.
#[derive(Debug, PartialEq)]
pub enum Command {
    Title(String),
    Color(u32),
    Resize(i32, i32),
    /// Fullscreen over its display, or back.
    Fullscreen(bool),
    Report,
    Close,
    Exit,
    /// A line that is not a command, with what is wrong with it.
    Invalid(String),
    Empty,
}

impl Command {
    pub fn parse(line: &str) -> Command {
        let mut words = line.split_whitespace();
        match words.next() {
            None => Command::Empty,
            Some("title") => Command::Title(
                line.trim_start()
                    .strip_prefix("title")
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            ),
            Some("color") => match words.next().and_then(parse_color) {
                Some(color) => Command::Color(color),
                None => Command::Invalid(String::from("color wants RRGGBB")),
            },
            Some("resize") => {
                let w = words.next().and_then(|s| s.parse().ok());
                let h = words.next().and_then(|s| s.parse().ok());
                match (w, h) {
                    (Some(w), Some(h)) => Command::Resize(w, h),
                    _ => Command::Invalid(String::from("resize wants W H")),
                }
            }
            Some("fullscreen") => match words.next() {
                Some("on") => Command::Fullscreen(true),
                Some("off") => Command::Fullscreen(false),
                _ => Command::Invalid(String::from("fullscreen wants on or off")),
            },
            Some("report") => Command::Report,
            Some("close") => Command::Close,
            Some("exit") => Command::Exit,
            Some(other) => Command::Invalid(format!("unknown {other}")),
        }
    }
}

/// One line of JSON on standard output, which is how a command is answered.
pub fn say(value: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    #[test]
    fn the_defaults_are_a_plain_window() {
        assert_eq!(parse(Vec::new()), Options::default());
    }

    #[test]
    fn every_option_is_read() {
        let options = parse(args(
            "--title a --class C --color #ff8000 --size 640x480 --min-size 100x50 --fixed \
             --rename-to b --after 1.5",
        ));
        assert_eq!(options.title, "a");
        assert_eq!(options.class.as_deref(), Some("C"));
        assert_eq!(options.color, Some(0xff8000));
        assert_eq!(options.size, (640, 480));
        assert_eq!(options.min_size, Some((100, 50)));
        assert!(options.fixed);
        assert_eq!(options.rename, Some((String::from("b"), 1.5)));
    }

    #[test]
    fn what_is_not_understood_is_left_out() {
        let options = parse(args("--size big --color nope --nothing --title t"));
        assert_eq!(options.size, (400, 300));
        assert_eq!(options.color, None);
        assert_eq!(options.title, "t");
    }

    #[test]
    fn a_colour_is_at_most_six_digits() {
        assert_eq!(parse_color("ffffff"), Some(0xff_ffff));
        assert_eq!(parse_color("1000000"), None);
    }

    #[test]
    fn commands_are_read() {
        assert_eq!(
            Command::parse("title two  words"),
            Command::Title(String::from("two  words"))
        );
        assert_eq!(Command::parse("color 00ff00"), Command::Color(0x00ff00));
        assert_eq!(Command::parse("resize 10 20"), Command::Resize(10, 20));
        assert_eq!(Command::parse("fullscreen on"), Command::Fullscreen(true));
        assert_eq!(Command::parse("fullscreen off"), Command::Fullscreen(false));
        assert_eq!(Command::parse("report"), Command::Report);
        assert_eq!(Command::parse("close"), Command::Close);
        assert_eq!(Command::parse("exit"), Command::Exit);
        assert_eq!(Command::parse("   "), Command::Empty);
    }

    #[test]
    fn a_command_that_is_wrong_says_how() {
        assert!(matches!(Command::parse("resize 10"), Command::Invalid(_)));
        assert!(matches!(Command::parse("color zz"), Command::Invalid(_)));
        assert!(matches!(Command::parse("fullscreen"), Command::Invalid(_)));
        assert_eq!(
            Command::parse("jump"),
            Command::Invalid(String::from("unknown jump"))
        );
    }
}
