//! Keys as KDE stores them: Qt key codes, modifier flags, and the names they are written with.
//!
//! KDE's global shortcut daemon, kglobalacceld, stores a shortcut as a `QKeySequence`. Each key of
//! the sequence is one integer: a Qt key code (`Qt::Key`) with the modifier flags
//! (`Qt::KeyboardModifier`) or'ed in, so that Alt+H is `0x0800_0000 | 0x48`. A `.desktop` file
//! writes the same sequence as portable text, `Alt+H`. This module is the bridge from the key
//! names of the three file formats to those integers and that text:
//!
//! - [`key_to_qt`] returns the [`Key`] a name stands for, in the vocabulary of one [`Format`].
//! - [`Mods`] are the modifier flags, and [`Combo`] is a key with its modifiers, in the form KWin
//!   matches.
//! - [`Combo::to_qt`] is the integer kglobalacceld stores, and [`Combo`]'s `Display` the portable
//!   text.
//!
//! ```rust
//! use komorebi_hotkeys::Format;
//! use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
//!
//! let h = key_to_qt(Format::Skhd, "h").unwrap();
//! let alt_h = Combo::new(Mods::ALT, h);
//! assert_eq!(alt_h.to_string(), "Alt+H");
//! assert_eq!(alt_h.to_qt(), 0x0800_0048);
//! ```
//!
//! # The shifted symbol rule
//!
//! KWin matches the character a key press produced, with the modifiers that produced it taken
//! away. Alt+Shift+1 arrives as Alt+!, so Alt+! is what has to be registered. Shifted letters and
//! named keys keep their Shift: Alt+Shift+H and Alt+Shift+Return arrive as they are. (Measured on
//! KWin 6.8.1 with a US layout.) [`Combo::new`] applies the rule, so every [`Combo`] is already in
//! the form KWin matches:
//!
//! ```rust
//! use komorebi_hotkeys::Format;
//! use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
//!
//! let key = |name| key_to_qt(Format::Skhd, name).unwrap();
//! assert_eq!(Combo::new(Mods::ALT | Mods::SHIFT, key("1")).to_string(), "Alt+!");
//! assert_eq!(Combo::new(Mods::ALT | Mods::SHIFT, key("h")).to_string(), "Alt+Shift+H");
//! ```
//!
//! # A US layout only
//!
//! Some names are positions on a keyboard, not characters: skhd's macOS keycodes (`0x21` is the
//! key right of `P`) and whkd's `oem_*` keys. They, and the shifted symbol rule, are read as on a
//! US layout. On another layout the key a position types, and the symbol Shift gives it, differ, and
//! a binding read from such a name may not be the key the user meant. No other layout is supported.

use crate::Format;
use std::fmt;
use std::ops::BitOr;
use std::ops::BitOrAssign;

/// A set of modifiers, as the flags Qt or's into a key code.
///
/// The four modifiers are the ones KDE knows. Side-specific modifiers (left Alt, right Alt) do
/// not exist here: KDE does not tell them apart in a shortcut.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::qt::Mods;
///
/// let mods = Mods::CTRL | Mods::ALT;
/// assert!(mods.contains(Mods::ALT));
/// assert!(!mods.contains(Mods::SHIFT));
/// assert_eq!(mods.bits(), 0x0c00_0000);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mods(u32);

impl Mods {
    /// No modifier.
    pub const NONE: Mods = Mods(0);
    /// Shift (`Qt::ShiftModifier`).
    pub const SHIFT: Mods = Mods(0x0200_0000);
    /// Control (`Qt::ControlModifier`).
    pub const CTRL: Mods = Mods(0x0400_0000);
    /// Alt (`Qt::AltModifier`).
    pub const ALT: Mods = Mods(0x0800_0000);
    /// The Super, Windows or Command key, which Qt calls Meta (`Qt::MetaModifier`).
    pub const META: Mods = Mods(0x1000_0000);

    /// Returns the flags, as Qt or's them into a key code.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Returns true if and only if every modifier in `other` is in `self`.
    pub const fn contains(self, other: Mods) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns true if and only if there is no modifier in the set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns the modifiers of `self` that are not in `other`.
    pub const fn without(self, other: Mods) -> Mods {
        Mods(self.0 & !other.0)
    }
}

impl BitOr for Mods {
    type Output = Mods;

    fn bitor(self, rhs: Mods) -> Mods {
        Mods(self.0 | rhs.0)
    }
}

impl BitOrAssign for Mods {
    fn bitor_assign(&mut self, rhs: Mods) {
        self.0 |= rhs.0;
    }
}

/// A key, as a Qt key code (`Qt::Key`).
///
/// Values come from [`key_to_qt`] or the constants. Every `Key` has a name in Qt's portable text,
/// which its `Display` writes: `H`, `[`, `Return`, `Volume Up`.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::Format;
/// use komorebi_hotkeys::qt::{Key, key_to_qt};
///
/// assert_eq!(key_to_qt(Format::Sxhkd, "Escape"), Some(Key::ESCAPE));
/// assert_eq!(Key::ESCAPE.code(), 0x0100_0000);
/// assert_eq!(Key::ESCAPE.to_string(), "Esc");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(u32);

impl Key {
    /// The Escape key (`Qt::Key_Escape`).
    pub const ESCAPE: Key = Key(0x0100_0000);

    /// Returns the Qt key code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            0x20 => f.write_str("Space"),
            code @ 0x21..=0x7e => {
                let c = char::from_u32(code).unwrap_or('?');
                write!(f, "{c}")
            }
            code => {
                let name = NAMED
                    .iter()
                    .find(|(c, _)| *c == code)
                    .map(|(_, name)| *name)
                    .or_else(|| function_key_name(code));
                match name {
                    Some(name) => f.write_str(name),
                    None => write!(f, "{code:#x}"),
                }
            }
        }
    }
}

/// A key with its modifiers, in the form KWin matches.
///
/// It is guaranteed that a `Combo` has the [shifted symbol rule](self#the-shifted-symbol-rule)
/// applied: there is no `Combo` with Shift on a key that types a different symbol with Shift. The
/// fields are private and [`Combo::new`] is the only way to make one, which is what holds the
/// guarantee. Two combos are therefore equal if and only if KWin cannot tell them apart.
///
/// A combo written field by field, which would skip the rule, does not compile:
///
/// ```rust,compile_fail,E0451
/// use komorebi_hotkeys::Format;
/// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
///
/// let one = key_to_qt(Format::Skhd, "1").unwrap();
/// let combo = Combo { mods: Mods::ALT | Mods::SHIFT, key: one };
/// ```
///
/// [`Combo::new`] is the way to make it:
///
/// ```rust
/// use komorebi_hotkeys::Format;
/// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
///
/// let one = key_to_qt(Format::Skhd, "1").unwrap();
/// let combo = Combo::new(Mods::ALT | Mods::SHIFT, one);
/// assert_eq!(combo.to_string(), "Alt+!");
/// ```
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::Format;
/// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
///
/// let one = key_to_qt(Format::Whkd, "1").unwrap();
/// let exclam = key_to_qt(Format::Sxhkd, "exclam").unwrap();
/// // Shift+1 types !, so the two are the same combo
/// assert_eq!(
///     Combo::new(Mods::ALT | Mods::SHIFT, one),
///     Combo::new(Mods::ALT, exclam),
/// );
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Combo {
    mods: Mods,
    key: Key,
}

impl Combo {
    /// Returns the combo KWin sees when `key` is pressed with `mods` held.
    ///
    /// With Shift held, a key that types a different symbol with Shift (the digits and the
    /// punctuation of a US layout) becomes that symbol, and Shift is taken away. Shift on the
    /// symbol itself is taken away as well, since typing the symbol needs it anyway.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_hotkeys::Format;
    /// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
    ///
    /// let key = |name| key_to_qt(Format::Skhd, name).unwrap();
    /// let combo = Combo::new(Mods::CTRL | Mods::ALT | Mods::SHIFT, key("9"));
    /// assert_eq!(combo.mods(), Mods::CTRL | Mods::ALT);
    /// assert_eq!(combo.to_string(), "Ctrl+Alt+(");
    /// // named keys keep their Shift
    /// assert_eq!(Combo::new(Mods::SHIFT, key("left")).to_string(), "Shift+Left");
    /// ```
    pub fn new(mods: Mods, key: Key) -> Combo {
        if mods.contains(Mods::SHIFT) {
            if let Some(shifted) = shifted_symbol(key) {
                return Combo {
                    mods: mods.without(Mods::SHIFT),
                    key: shifted,
                };
            }
            if SHIFTED
                .iter()
                .any(|(_, shifted)| key.0 == u32::from(*shifted))
            {
                return Combo {
                    mods: mods.without(Mods::SHIFT),
                    key,
                };
            }
        }
        Combo { mods, key }
    }

    /// Returns the modifiers.
    pub const fn mods(self) -> Mods {
        self.mods
    }

    /// Returns the key.
    pub const fn key(self) -> Key {
        self.key
    }

    /// Returns the integer kglobalacceld stores for this combo: the key code with the modifier
    /// flags or'ed in.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_hotkeys::Format;
    /// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
    ///
    /// let left = key_to_qt(Format::Whkd, "left").unwrap();
    /// assert_eq!(Combo::new(Mods::CTRL | Mods::ALT, left).to_qt(), 0x0c00_0000 | 0x0100_0012);
    /// ```
    pub const fn to_qt(self) -> u32 {
        self.mods.0 | self.key.0
    }
}

/// Writes the combo in Qt's portable text, the form of an `X-KDE-Shortcuts` value: the modifiers
/// in Qt's order (`Meta`, `Ctrl`, `Alt`, `Shift`), each followed by `+`, then the key.
impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (flag, name) in [
            (Mods::META, "Meta"),
            (Mods::CTRL, "Ctrl"),
            (Mods::ALT, "Alt"),
            (Mods::SHIFT, "Shift"),
        ] {
            if self.mods.contains(flag) {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.key)
    }
}

/// Returns the key a name stands for in the vocabulary of a format, or `None` when the name is
/// not a key this crate knows.
///
/// Every format shares the letters (`a` or `A`), the digits and the function keys `f1` to `f24`.
/// The rest of each vocabulary is the format's own:
///
/// - **skhd**: its literal keywords (`return`, `escape`, `pageup`, `sound_up`, `play`...), a
///   macOS keycode in hex (`0x21`, read as the key at that position on a US keyboard), and the
///   punctuation characters ``[ ] ; = - ' . / \ ` ,``.
/// - **whkd**: Windows virtual key names, with or without `VK_` (`return`, `back`, `prior`,
///   `oem_4`, `volume_up`...), read as on a US keyboard.
/// - **sxhkd**: X keysym names (`Return`, `BackSpace`, `bracketleft`, `exclam`, `XF86AudioPlay`...).
///
/// Names are matched without regard to case. Keypad keys, mouse buttons and the keys Qt has no
/// portable name for are not known.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::Format;
/// use komorebi_hotkeys::qt::key_to_qt;
///
/// // the key right of P, in all three vocabularies
/// let bracket = key_to_qt(Format::Skhd, "0x21");
/// assert_eq!(bracket.map(|k| k.to_string()).as_deref(), Some("["));
/// assert_eq!(key_to_qt(Format::Whkd, "oem_4"), bracket);
/// assert_eq!(key_to_qt(Format::Sxhkd, "bracketleft"), bracket);
///
/// // a name means what its format says: whkd's `back` is Backspace
/// assert_eq!(key_to_qt(Format::Whkd, "back").unwrap().to_string(), "Backspace");
/// assert_eq!(key_to_qt(Format::Sxhkd, "XF86Back").unwrap().to_string(), "Back");
/// assert_eq!(key_to_qt(Format::Skhd, "0xFF"), None);
/// ```
pub fn key_to_qt(format: Format, name: &str) -> Option<Key> {
    let name = match format {
        Format::Whkd if name.len() > 3 && name[..3].eq_ignore_ascii_case("vk_") => &name[3..],
        _ => name,
    };

    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(Key(u32::from(c.to_ascii_uppercase())));
    }

    let lower = name.to_ascii_lowercase();
    if let Some(digits) = lower.strip_prefix('f')
        && !digits.starts_with('0')
        && digits.bytes().all(|b| b.is_ascii_digit())
        && let Ok(n) = digits.parse::<u32>()
        && (1..=24).contains(&n)
    {
        return Some(Key(F1 + n - 1));
    }

    let code = match format {
        Format::Skhd => {
            if let Some(hex) = lower.strip_prefix("0x") {
                if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                let code = u32::from_str_radix(hex, 16).ok()?;
                return macos_keycode(code).map(Key);
            }
            lookup(SKHD, &lower)
        }
        Format::Whkd => lookup(WHKD, &lower),
        Format::Sxhkd => lookup(SXHKD, &lower),
    };
    code.map(Key)
}

fn lookup(table: &[(&str, u32)], name: &str) -> Option<u32> {
    table
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, code)| *code)
}

/// The symbol Shift and this key type on a US keyboard, for the keys whose symbol changes.
fn shifted_symbol(key: Key) -> Option<Key> {
    SHIFTED
        .iter()
        .find(|(plain, _)| key.0 == u32::from(*plain))
        .map(|(_, shifted)| Key(u32::from(*shifted)))
}

/// The keys of a US keyboard that type a different symbol with Shift, and that symbol.
const SHIFTED: &[(char, char)] = &[
    ('1', '!'),
    ('2', '@'),
    ('3', '#'),
    ('4', '$'),
    ('5', '%'),
    ('6', '^'),
    ('7', '&'),
    ('8', '*'),
    ('9', '('),
    ('0', ')'),
    ('-', '_'),
    ('=', '+'),
    ('[', '{'),
    (']', '}'),
    (';', ':'),
    ('\'', '"'),
    (',', '<'),
    ('.', '>'),
    ('/', '?'),
    ('\\', '|'),
    ('`', '~'),
];

const F1: u32 = 0x0100_0030;
const ESCAPE: u32 = 0x0100_0000;
const TAB: u32 = 0x0100_0001;
const BACKSPACE: u32 = 0x0100_0003;
const RETURN: u32 = 0x0100_0004;
const INSERT: u32 = 0x0100_0006;
const DELETE: u32 = 0x0100_0007;
const PAUSE: u32 = 0x0100_0008;
const PRINT: u32 = 0x0100_0009;
const SYSREQ: u32 = 0x0100_000a;
const CLEAR: u32 = 0x0100_000b;
const HOME: u32 = 0x0100_0010;
const END: u32 = 0x0100_0011;
const LEFT: u32 = 0x0100_0012;
const UP: u32 = 0x0100_0013;
const RIGHT: u32 = 0x0100_0014;
const DOWN: u32 = 0x0100_0015;
const PAGE_UP: u32 = 0x0100_0016;
const PAGE_DOWN: u32 = 0x0100_0017;
const CAPS_LOCK: u32 = 0x0100_0024;
const NUM_LOCK: u32 = 0x0100_0025;
const SCROLL_LOCK: u32 = 0x0100_0026;
const MENU: u32 = 0x0100_0055;
const HELP: u32 = 0x0100_0058;
const BACK: u32 = 0x0100_0061;
const FORWARD: u32 = 0x0100_0062;
const STOP: u32 = 0x0100_0063;
const REFRESH: u32 = 0x0100_0064;
const VOLUME_DOWN: u32 = 0x0100_0070;
const VOLUME_MUTE: u32 = 0x0100_0071;
const VOLUME_UP: u32 = 0x0100_0072;
const MEDIA_PLAY: u32 = 0x0100_0080;
const MEDIA_STOP: u32 = 0x0100_0081;
const MEDIA_PREVIOUS: u32 = 0x0100_0082;
const MEDIA_NEXT: u32 = 0x0100_0083;
const MEDIA_RECORD: u32 = 0x0100_0084;
const MEDIA_PAUSE: u32 = 0x0100_0085;
const HOME_PAGE: u32 = 0x0100_0090;
const FAVORITES: u32 = 0x0100_0091;
const SEARCH: u32 = 0x0100_0092;
const LAUNCH_MAIL: u32 = 0x0100_00a0;
const LAUNCH_MEDIA: u32 = 0x0100_00a1;
const MON_BRIGHTNESS_UP: u32 = 0x0100_00b2;
const MON_BRIGHTNESS_DOWN: u32 = 0x0100_00b3;
const KEYBOARD_LIGHT_ON_OFF: u32 = 0x0100_00b4;
const KEYBOARD_BRIGHTNESS_UP: u32 = 0x0100_00b5;
const KEYBOARD_BRIGHTNESS_DOWN: u32 = 0x0100_00b6;
const POWER_OFF: u32 = 0x0100_00b7;
const WAKE_UP: u32 = 0x0100_00b8;
const EJECT: u32 = 0x0100_00b9;
const SCREEN_SAVER: u32 = 0x0100_00ba;
const WWW: u32 = 0x0100_00bb;
const AUDIO_REWIND: u32 = 0x0100_00c5;
const CALCULATOR: u32 = 0x0100_00cb;
const DISPLAY: u32 = 0x0100_00d1;
const EXPLORER: u32 = 0x0100_00d5;
const CALENDAR: u32 = 0x0100_00e4;
const TERMINAL: u32 = 0x0100_00f0;
const TOOLS: u32 = 0x0100_00f1;
const AUDIO_FORWARD: u32 = 0x0100_0102;
const MIC_MUTE: u32 = 0x0100_0113;
const SELECT: u32 = 0x0101_0000;
const SLEEP: u32 = 0x0102_0004;

/// Qt's portable text for the keys outside printable ASCII that a vocabulary below can name.
const NAMED: &[(u32, &str)] = &[
    (ESCAPE, "Esc"),
    (TAB, "Tab"),
    (BACKSPACE, "Backspace"),
    (RETURN, "Return"),
    (INSERT, "Ins"),
    (DELETE, "Del"),
    (PAUSE, "Pause"),
    (PRINT, "Print"),
    (SYSREQ, "SysReq"),
    (CLEAR, "Clear"),
    (HOME, "Home"),
    (END, "End"),
    (LEFT, "Left"),
    (UP, "Up"),
    (RIGHT, "Right"),
    (DOWN, "Down"),
    (PAGE_UP, "PgUp"),
    (PAGE_DOWN, "PgDown"),
    (CAPS_LOCK, "CapsLock"),
    (NUM_LOCK, "NumLock"),
    (SCROLL_LOCK, "ScrollLock"),
    (MENU, "Menu"),
    (HELP, "Help"),
    (BACK, "Back"),
    (FORWARD, "Forward"),
    (STOP, "Stop"),
    (REFRESH, "Refresh"),
    (VOLUME_DOWN, "Volume Down"),
    (VOLUME_MUTE, "Volume Mute"),
    (VOLUME_UP, "Volume Up"),
    (MEDIA_PLAY, "Media Play"),
    (MEDIA_STOP, "Media Stop"),
    (MEDIA_PREVIOUS, "Media Previous"),
    (MEDIA_NEXT, "Media Next"),
    (MEDIA_RECORD, "Media Record"),
    (MEDIA_PAUSE, "Media Pause"),
    (HOME_PAGE, "Home Page"),
    (FAVORITES, "Favorites"),
    (SEARCH, "Search"),
    (LAUNCH_MAIL, "Launch Mail"),
    (LAUNCH_MEDIA, "Launch Media"),
    (MON_BRIGHTNESS_UP, "Monitor Brightness Up"),
    (MON_BRIGHTNESS_DOWN, "Monitor Brightness Down"),
    (KEYBOARD_LIGHT_ON_OFF, "Keyboard Light On/Off"),
    (KEYBOARD_BRIGHTNESS_UP, "Keyboard Brightness Up"),
    (KEYBOARD_BRIGHTNESS_DOWN, "Keyboard Brightness Down"),
    (POWER_OFF, "Power Off"),
    (WAKE_UP, "Wake Up"),
    (EJECT, "Eject"),
    (SCREEN_SAVER, "Screensaver"),
    (WWW, "WWW"),
    (AUDIO_REWIND, "Media Rewind"),
    (CALCULATOR, "Calculator"),
    (DISPLAY, "Display"),
    (EXPLORER, "Browser"),
    (CALENDAR, "Calendar"),
    (TERMINAL, "Terminal"),
    (TOOLS, "Tools"),
    (AUDIO_FORWARD, "Media Fast Forward"),
    (MIC_MUTE, "Microphone Mute"),
    (SELECT, "Select"),
    (SLEEP, "Sleep"),
];

fn function_key_name(code: u32) -> Option<&'static str> {
    const NAMES: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
        "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    let index = code.checked_sub(F1)?;
    NAMES.get(usize::try_from(index).ok()?).copied()
}

const fn ch(c: char) -> u32 {
    c as u32
}

/// skhd's literal keywords and the punctuation characters, in lower case.
const SKHD: &[(&str, u32)] = &[
    ("return", RETURN),
    ("enter", RETURN),
    ("tab", TAB),
    ("space", 0x20),
    ("backspace", BACKSPACE),
    ("delete", DELETE),
    ("escape", ESCAPE),
    ("home", HOME),
    ("end", END),
    ("pageup", PAGE_UP),
    ("pagedown", PAGE_DOWN),
    ("insert", INSERT),
    ("left", LEFT),
    ("right", RIGHT),
    ("up", UP),
    ("down", DOWN),
    ("sound_up", VOLUME_UP),
    ("sound_down", VOLUME_DOWN),
    ("mute", VOLUME_MUTE),
    ("brightness_up", MON_BRIGHTNESS_UP),
    ("brightness_down", MON_BRIGHTNESS_DOWN),
    ("illumination_up", KEYBOARD_BRIGHTNESS_UP),
    ("illumination_down", KEYBOARD_BRIGHTNESS_DOWN),
    ("play", MEDIA_PLAY),
    ("previous", MEDIA_PREVIOUS),
    ("next", MEDIA_NEXT),
    ("rewind", AUDIO_REWIND),
    ("fast", AUDIO_FORWARD),
    ("[", ch('[')),
    ("]", ch(']')),
    (";", ch(';')),
    ("=", ch('=')),
    ("-", ch('-')),
    ("'", ch('\'')),
    (".", ch('.')),
    ("/", ch('/')),
    ("\\", ch('\\')),
    ("`", ch('`')),
    (",", ch(',')),
];

/// The macOS virtual keycodes (`kVK_ANSI_*` and the named keys) skhd takes in hex, as the key at
/// that position on a US keyboard.
fn macos_keycode(code: u32) -> Option<u32> {
    Some(match code {
        0x00 => ch('A'),
        0x01 => ch('S'),
        0x02 => ch('D'),
        0x03 => ch('F'),
        0x04 => ch('H'),
        0x05 => ch('G'),
        0x06 => ch('Z'),
        0x07 => ch('X'),
        0x08 => ch('C'),
        0x09 => ch('V'),
        0x0B => ch('B'),
        0x0C => ch('Q'),
        0x0D => ch('W'),
        0x0E => ch('E'),
        0x0F => ch('R'),
        0x10 => ch('Y'),
        0x11 => ch('T'),
        0x12 => ch('1'),
        0x13 => ch('2'),
        0x14 => ch('3'),
        0x15 => ch('4'),
        0x16 => ch('6'),
        0x17 => ch('5'),
        0x18 => ch('='),
        0x19 => ch('9'),
        0x1A => ch('7'),
        0x1B => ch('-'),
        0x1C => ch('8'),
        0x1D => ch('0'),
        0x1E => ch(']'),
        0x1F => ch('O'),
        0x20 => ch('U'),
        0x21 => ch('['),
        0x22 => ch('I'),
        0x23 => ch('P'),
        0x24 => RETURN,
        0x25 => ch('L'),
        0x26 => ch('J'),
        0x27 => ch('\''),
        0x28 => ch('K'),
        0x29 => ch(';'),
        0x2A => ch('\\'),
        0x2B => ch(','),
        0x2C => ch('/'),
        0x2D => ch('N'),
        0x2E => ch('M'),
        0x2F => ch('.'),
        0x30 => TAB,
        0x31 => 0x20,
        0x32 => ch('`'),
        0x33 => BACKSPACE,
        0x35 => ESCAPE,
        0x73 => HOME,
        0x74 => PAGE_UP,
        0x75 => DELETE,
        0x77 => END,
        0x79 => PAGE_DOWN,
        0x7B => LEFT,
        0x7C => RIGHT,
        0x7D => DOWN,
        0x7E => UP,
        0x7A => F1,
        0x78 => F1 + 1,
        0x63 => F1 + 2,
        0x76 => F1 + 3,
        0x60 => F1 + 4,
        0x61 => F1 + 5,
        0x62 => F1 + 6,
        0x64 => F1 + 7,
        0x65 => F1 + 8,
        0x6D => F1 + 9,
        0x67 => F1 + 10,
        0x6F => F1 + 11,
        _ => return None,
    })
}

/// The Windows virtual key names whkd accepts that are not letters, digits or function keys,
/// without their `VK_` prefix.
const WHKD: &[(&str, u32)] = &[
    ("back", BACKSPACE),
    ("tab", TAB),
    ("clear", CLEAR),
    ("return", RETURN),
    ("pause", PAUSE),
    ("capital", CAPS_LOCK),
    ("escape", ESCAPE),
    ("space", 0x20),
    ("prior", PAGE_UP),
    ("next", PAGE_DOWN),
    ("end", END),
    ("home", HOME),
    ("left", LEFT),
    ("up", UP),
    ("right", RIGHT),
    ("down", DOWN),
    ("select", SELECT),
    ("snapshot", PRINT),
    ("insert", INSERT),
    ("delete", DELETE),
    ("help", HELP),
    ("apps", MENU),
    ("sleep", SLEEP),
    ("numlock", NUM_LOCK),
    ("scroll", SCROLL_LOCK),
    ("browser_back", BACK),
    ("browser_forward", FORWARD),
    ("browser_refresh", REFRESH),
    ("browser_stop", STOP),
    ("browser_search", SEARCH),
    ("browser_favorites", FAVORITES),
    ("browser_home", HOME_PAGE),
    ("volume_mute", VOLUME_MUTE),
    ("volume_down", VOLUME_DOWN),
    ("volume_up", VOLUME_UP),
    ("media_next_track", MEDIA_NEXT),
    ("media_prev_track", MEDIA_PREVIOUS),
    ("media_stop", MEDIA_STOP),
    ("media_play_pause", MEDIA_PLAY),
    ("launch_mail", LAUNCH_MAIL),
    ("launch_media_select", LAUNCH_MEDIA),
    ("oem_1", ch(';')),
    ("oem_plus", ch('=')),
    ("oem_comma", ch(',')),
    ("oem_minus", ch('-')),
    ("oem_period", ch('.')),
    ("oem_2", ch('/')),
    ("oem_3", ch('`')),
    ("oem_4", ch('[')),
    ("oem_5", ch('\\')),
    ("oem_6", ch(']')),
    ("oem_7", ch('\'')),
];

/// The X keysym names sxhkd accepts that are not letters, digits or function keys, mapped the way
/// Qt maps the keysym.
const SXHKD: &[(&str, u32)] = &[
    ("Return", RETURN),
    ("Tab", TAB),
    ("space", 0x20),
    ("BackSpace", BACKSPACE),
    ("Escape", ESCAPE),
    ("Delete", DELETE),
    ("Insert", INSERT),
    ("Home", HOME),
    ("End", END),
    ("Prior", PAGE_UP),
    ("Page_Up", PAGE_UP),
    ("Next", PAGE_DOWN),
    ("Page_Down", PAGE_DOWN),
    ("Left", LEFT),
    ("Up", UP),
    ("Right", RIGHT),
    ("Down", DOWN),
    ("Print", PRINT),
    ("Pause", PAUSE),
    ("Sys_Req", SYSREQ),
    ("Scroll_Lock", SCROLL_LOCK),
    ("Caps_Lock", CAPS_LOCK),
    ("Num_Lock", NUM_LOCK),
    ("Menu", MENU),
    ("Help", HELP),
    ("exclam", ch('!')),
    ("quotedbl", ch('"')),
    ("numbersign", ch('#')),
    ("dollar", ch('$')),
    ("percent", ch('%')),
    ("ampersand", ch('&')),
    ("apostrophe", ch('\'')),
    ("quoteright", ch('\'')),
    ("parenleft", ch('(')),
    ("parenright", ch(')')),
    ("asterisk", ch('*')),
    ("plus", ch('+')),
    ("comma", ch(',')),
    ("minus", ch('-')),
    ("period", ch('.')),
    ("slash", ch('/')),
    ("colon", ch(':')),
    ("semicolon", ch(';')),
    ("less", ch('<')),
    ("equal", ch('=')),
    ("greater", ch('>')),
    ("question", ch('?')),
    ("at", ch('@')),
    ("bracketleft", ch('[')),
    ("backslash", ch('\\')),
    ("bracketright", ch(']')),
    ("asciicircum", ch('^')),
    ("underscore", ch('_')),
    ("grave", ch('`')),
    ("quoteleft", ch('`')),
    ("braceleft", ch('{')),
    ("bar", ch('|')),
    ("braceright", ch('}')),
    ("asciitilde", ch('~')),
    ("XF86AudioRaiseVolume", VOLUME_UP),
    ("XF86AudioLowerVolume", VOLUME_DOWN),
    ("XF86AudioMute", VOLUME_MUTE),
    ("XF86AudioMicMute", MIC_MUTE),
    ("XF86AudioPlay", MEDIA_PLAY),
    ("XF86AudioStop", MEDIA_STOP),
    ("XF86AudioPrev", MEDIA_PREVIOUS),
    ("XF86AudioNext", MEDIA_NEXT),
    ("XF86AudioPause", MEDIA_PAUSE),
    ("XF86AudioRecord", MEDIA_RECORD),
    ("XF86AudioRewind", AUDIO_REWIND),
    ("XF86AudioForward", AUDIO_FORWARD),
    ("XF86AudioMedia", LAUNCH_MEDIA),
    ("XF86MonBrightnessUp", MON_BRIGHTNESS_UP),
    ("XF86MonBrightnessDown", MON_BRIGHTNESS_DOWN),
    ("XF86KbdLightOnOff", KEYBOARD_LIGHT_ON_OFF),
    ("XF86KbdBrightnessUp", KEYBOARD_BRIGHTNESS_UP),
    ("XF86KbdBrightnessDown", KEYBOARD_BRIGHTNESS_DOWN),
    ("XF86Back", BACK),
    ("XF86Forward", FORWARD),
    ("XF86Stop", STOP),
    ("XF86Refresh", REFRESH),
    ("XF86Favorites", FAVORITES),
    ("XF86HomePage", HOME_PAGE),
    ("XF86Search", SEARCH),
    ("XF86Mail", LAUNCH_MAIL),
    ("XF86Calculator", CALCULATOR),
    ("XF86Calendar", CALENDAR),
    ("XF86Explorer", EXPLORER),
    ("XF86WWW", WWW),
    ("XF86Terminal", TERMINAL),
    ("XF86Tools", TOOLS),
    ("XF86Display", DISPLAY),
    ("XF86ScreenSaver", SCREEN_SAVER),
    ("XF86Eject", EJECT),
    ("XF86Sleep", SLEEP),
    ("XF86PowerOff", POWER_OFF),
    ("XF86WakeUp", WAKE_UP),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(format: Format, mods: Mods, name: &str) -> String {
        Combo::new(mods, key_to_qt(format, name).unwrap()).to_string()
    }

    #[test]
    fn modifiers_are_written_in_the_order_qt_writes_them() {
        let all = Mods::SHIFT | Mods::ALT | Mods::CTRL | Mods::META;
        assert_eq!(combo(Format::Skhd, all, "a"), "Meta+Ctrl+Alt+Shift+A");
        assert_eq!(
            combo(Format::Skhd, Mods::ALT | Mods::SHIFT, "return"),
            "Alt+Shift+Return"
        );
        assert_eq!(
            combo(Format::Skhd, Mods::CTRL | Mods::ALT, "left"),
            "Ctrl+Alt+Left"
        );
        assert_eq!(combo(Format::Skhd, Mods::ALT, "1"), "Alt+1");
    }

    #[test]
    fn macos_keycodes_become_the_key_they_are() {
        let alt_shift = Mods::ALT | Mods::SHIFT;
        assert_eq!(combo(Format::Skhd, alt_shift, "0x21"), "Alt+{");
        assert_eq!(combo(Format::Skhd, alt_shift, "0x1E"), "Alt+}");
        assert_eq!(combo(Format::Skhd, Mods::ALT, "0x29"), "Alt+;");
        assert_eq!(combo(Format::Skhd, Mods::ALT, "0x18"), "Alt+=");
        assert_eq!(combo(Format::Skhd, Mods::ALT, "0x1B"), "Alt+-");
        assert_eq!(key_to_qt(Format::Skhd, "0xFF"), None);
        assert_eq!(key_to_qt(Format::Skhd, "0xzz"), None);
    }

    #[test]
    fn shift_on_a_symbol_key_is_the_symbol_it_types() {
        let alt_shift = Mods::ALT | Mods::SHIFT;
        assert_eq!(combo(Format::Skhd, alt_shift, "1"), "Alt+!");
        assert_eq!(combo(Format::Skhd, alt_shift, "0x29"), "Alt+:");
        assert_eq!(combo(Format::Skhd, alt_shift, "0x18"), "Alt++");
        assert_eq!(combo(Format::Skhd, alt_shift, "0x1B"), "Alt+_");
        assert_eq!(
            combo(Format::Skhd, Mods::CTRL | alt_shift, "9"),
            "Ctrl+Alt+("
        );
        // letters and named keys keep their Shift
        assert_eq!(combo(Format::Skhd, alt_shift, "h"), "Alt+Shift+H");
        assert_eq!(combo(Format::Skhd, alt_shift, "return"), "Alt+Shift+Return");
        assert_eq!(
            combo(Format::Skhd, Mods::CTRL | Mods::SHIFT, "left"),
            "Ctrl+Shift+Left"
        );
        // Shift on the symbol itself goes too
        assert_eq!(combo(Format::Sxhkd, alt_shift, "exclam"), "Alt+!");
        assert_eq!(combo(Format::Sxhkd, Mods::ALT, "exclam"), "Alt+!");
    }

    #[test]
    fn every_shifted_pair_follows_the_rule() {
        for (plain, shifted) in SHIFTED {
            let combo = Combo::new(Mods::META | Mods::SHIFT, Key(u32::from(*plain)));
            assert_eq!(combo.to_string(), format!("Meta+{shifted}"), "{plain}");
        }
    }

    #[test]
    fn the_integer_is_the_key_with_the_flags() {
        let h = key_to_qt(Format::Whkd, "h").unwrap();
        assert_eq!(Combo::new(Mods::ALT, h).to_qt(), 0x0800_0048);
        let f12 = key_to_qt(Format::Sxhkd, "F12").unwrap();
        assert_eq!(
            Combo::new(Mods::META, f12).to_qt(),
            0x1000_0000 | 0x0100_003b
        );
        assert_eq!(
            Combo::new(
                Mods::ALT | Mods::SHIFT,
                key_to_qt(Format::Skhd, "1").unwrap()
            )
            .to_qt(),
            0x0800_0000 | 0x21
        );
    }

    #[test]
    fn the_three_vocabularies_name_the_same_keys() {
        for (skhd, whkd, sxhkd, text) in [
            ("return", "return", "Return", "Return"),
            ("escape", "escape", "Escape", "Esc"),
            ("backspace", "back", "BackSpace", "Backspace"),
            ("delete", "delete", "Delete", "Del"),
            ("pageup", "prior", "Prior", "PgUp"),
            ("pagedown", "next", "Next", "PgDown"),
            ("space", "space", "space", "Space"),
            ("0x29", "oem_1", "semicolon", ";"),
            ("0x27", "oem_7", "apostrophe", "'"),
            ("0x32", "oem_3", "grave", "`"),
            ("0x2A", "oem_5", "backslash", "\\"),
            ("0x2C", "oem_2", "slash", "/"),
            ("0x2F", "oem_period", "period", "."),
            ("0x2B", "oem_comma", "comma", ","),
            ("f1", "F1", "F1", "F1"),
            ("0x6F", "f12", "F12", "F12"),
            ("sound_up", "volume_up", "XF86AudioRaiseVolume", "Volume Up"),
            ("play", "media_play_pause", "XF86AudioPlay", "Media Play"),
        ] {
            let s = key_to_qt(Format::Skhd, skhd).unwrap();
            assert_eq!(key_to_qt(Format::Whkd, whkd), Some(s), "{whkd}");
            assert_eq!(key_to_qt(Format::Sxhkd, sxhkd), Some(s), "{sxhkd}");
            assert_eq!(s.to_string(), text);
        }
    }

    #[test]
    fn names_are_matched_without_regard_to_case() {
        assert_eq!(
            key_to_qt(Format::Whkd, "VK_OEM_4"),
            key_to_qt(Format::Whkd, "oem_4")
        );
        assert_eq!(
            key_to_qt(Format::Sxhkd, "return"),
            key_to_qt(Format::Sxhkd, "Return")
        );
        assert_eq!(key_to_qt(Format::Skhd, "H"), key_to_qt(Format::Skhd, "h"));
        // whkd takes every name with or without `VK_`
        assert_eq!(
            key_to_qt(Format::Whkd, "vk_a"),
            key_to_qt(Format::Whkd, "a")
        );
        assert_eq!(
            key_to_qt(Format::Whkd, "VK_0"),
            key_to_qt(Format::Whkd, "0")
        );
    }

    #[test]
    fn unknown_names_are_none() {
        for (format, name) in [
            (Format::Skhd, "f25"),
            (Format::Skhd, "f01"),
            (Format::Skhd, "fn"),
            (Format::Whkd, "numpad1"),
            (Format::Whkd, "0x21"),
            (Format::Sxhkd, "button1"),
            (Format::Sxhkd, "KP_1"),
            (Format::Sxhkd, "["),
            (Format::Whkd, ""),
        ] {
            assert_eq!(key_to_qt(format, name), None, "{format:?} {name}");
        }
    }

    #[test]
    fn every_named_key_has_portable_text() {
        let tables = [SKHD, WHKD, SXHKD];
        for (name, code) in tables.iter().flat_map(|t| t.iter()) {
            let text = Key(*code).to_string();
            assert!(!text.starts_with("0x"), "{name} has no name");
        }
        for n in 1..=24 {
            assert_eq!(
                key_to_qt(Format::Sxhkd, &format!("F{n}"))
                    .unwrap()
                    .to_string(),
                format!("F{n}")
            );
        }
    }
}
