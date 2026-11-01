//! The whkd reader against whkd's own parser, `whkd-parser`, on the whkdrc files of the corpus and
//! on the example of whkd's README: both must read the same bindings.

use komorebi_hotkeys::App;
use komorebi_hotkeys::Body;
use komorebi_hotkeys::Format;
use komorebi_hotkeys::ParseOptions;
use komorebi_hotkeys::Run;
use komorebi_hotkeys::Scope;
use komorebi_hotkeys::Then;
use komorebi_hotkeys::qt::Combo;
use komorebi_hotkeys::qt::Mods;
use komorebi_hotkeys::qt::key_to_qt;
use komorebi_hotkeys::whkd;
use std::path::Path;
use std::path::PathBuf;

/// The bindings of a file, as (keys, what they do), and the pause binding with its hook.
type Read = (Vec<(Combo, Body)>, Option<(Combo, Option<String>)>);

/// The combo a list of whkd key names stands for: the last is the key, the others modifiers.
fn combo(keys: &[String]) -> Combo {
    let (key, modifiers) = keys.split_last().unwrap();
    let mut mods = Mods::NONE;
    for modifier in modifiers {
        mods |= match modifier.to_ascii_lowercase().as_str() {
            "alt" => Mods::ALT,
            "ctrl" => Mods::CTRL,
            "shift" => Mods::SHIFT,
            "win" => Mods::META,
            other => panic!("the corpus has no modifier {other}"),
        };
    }
    Combo::new(mods, key_to_qt(Format::Whkd, key).unwrap())
}

/// What whkd-parser reads.
fn theirs(path: &Path) -> Read {
    let whkdrc = whkd_parser::load(&path.to_path_buf()).unwrap();
    let mut bindings = Vec::new();
    for (keys, apps) in &whkdrc.app_bindings {
        let scopes = apps
            .iter()
            .map(|binding| Scope {
                app: match binding.process_name.as_deref() {
                    Some("Default") => App::Other,
                    Some(name) => App::Named(name.to_string()),
                    None => panic!("an application binding with no application"),
                },
                then: match binding.command.as_str() {
                    "Ignore" => Then::Ignore,
                    command => Then::Run(Run::Command(command.to_string())),
                },
            })
            .collect();
        bindings.push((combo(keys), Body::Scoped(scopes)));
    }
    for binding in &whkdrc.bindings {
        bindings.push((
            combo(&binding.keys),
            Body::Run(Run::Command(binding.command.clone())),
        ));
    }
    let pause = whkdrc
        .pause_binding
        .as_ref()
        .map(|keys| (combo(keys), whkdrc.pause_hook.clone()));
    (bindings, pause)
}

/// What the whkd reader reads.
fn ours(path: &Path) -> Read {
    let text = std::fs::read_to_string(path).unwrap();
    let parsed = whkd::parse(&text, &ParseOptions::default());
    assert_eq!(parsed.keymap.modes.len(), 1);
    let bindings = parsed.keymap.modes[0]
        .bindings
        .iter()
        .map(|binding| (binding.chord[0], binding.body.clone()))
        .collect();
    let pause = parsed
        .keymap
        .pause
        .map(|pause| (pause.combo, pause.hook.clone()));
    (bindings, pause)
}

#[test]
fn every_whkdrc_of_the_corpus_reads_as_whkd_reads_it() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut compared = 0;
    for entry in std::fs::read_dir(fixtures).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "whkdrc") {
            assert_eq!(ours(&path), theirs(&path), "{}", path.display());
            compared += 1;
        }
    }
    assert_eq!(compared, 10);
}

#[test]
fn the_example_of_whkds_readme_reads_as_whkd_reads_it() {
    let example = r#".shell pwsh # can be one of cmd | pwsh | powershell
.pause alt + shift + p # can be any hotkey combo to toggle all other hotkeys on and off
.pause_hook echo "you can call whatever powershell command you want here"

alt + n [
    # ProcessName as shown by `Get-Process`
    Firefox       : echo "hello firefox"

    # Spaces are fine, no quotes required
    Google Chrome : echo "hello chrome"
]

alt + q [
    # Default is a keyword which will apply to all apps
    # If you only have Default, this is the same as doing "alt + q : komorebic close"
    Default       : komorebic close

    # Ignore is a keyword which will skip running the hotkey for the given process
    Google Chrome : Ignore
]

# focus windows with komorebi
alt + h : komorebic focus left
alt + j : komorebic focus down
alt + k : komorebic focus up
alt + l : komorebic focus right
"#;
    let path = std::env::temp_dir().join(format!(
        "komorebi-hotkeys-whkd-readme-{}.whkdrc",
        std::process::id()
    ));
    std::fs::write(&path, example).unwrap();
    let (ours, theirs) = (ours(&path), theirs(&path));
    let _ = std::fs::remove_file(&path);

    assert_eq!(ours.0.len(), 6);
    assert_eq!(ours, theirs);
}
