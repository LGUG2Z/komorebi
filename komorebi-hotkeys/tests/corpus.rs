//! The fixture corpus: key binding files that say the same thing in each format that can say it.
//!
//! Each case is a file per format in `tests/fixtures`, `<case>.skhdrc`, `<case>.whkdrc` and
//! `<case>.sxhkdrc`, for the formats that can express the case. The files of a case mean the same
//! keymap, and start with the same comment, which says what the case is about. [`EXPECTED`] is
//! that keymap, written out by [`render`].

use komorebi_hotkeys::App;
use komorebi_hotkeys::Body;
use komorebi_hotkeys::DEFAULT_MODE;
use komorebi_hotkeys::Edge;
use komorebi_hotkeys::Format;
use komorebi_hotkeys::Keymap;
use komorebi_hotkeys::ParseOptions;
use komorebi_hotkeys::Parsed;
use komorebi_hotkeys::Run;
use komorebi_hotkeys::Severity;
use komorebi_hotkeys::Span;
use komorebi_hotkeys::Then;
use komorebi_hotkeys::engine::Effect;
use komorebi_hotkeys::engine::Engine;
use komorebi_hotkeys::engine::EngineOptions;
use komorebi_hotkeys::kde::desktop_files;
use komorebi_hotkeys::plan::PlanMode;
use komorebi_hotkeys::plan::PlanOptions;
use komorebi_hotkeys::plan::plan;
use std::path::PathBuf;

/// Every case, and the formats it is written in.
const CASES: &[(&str, &[&str])] = &[
    ("plain", &["skhdrc", "whkdrc", "sxhkdrc"]),
    ("modifiers", &["skhdrc", "whkdrc", "sxhkdrc"]),
    ("keycodes", &["skhdrc", "whkdrc", "sxhkdrc"]),
    ("comments", &["skhdrc", "whkdrc", "sxhkdrc"]),
    ("braces", &["skhdrc", "whkdrc", "sxhkdrc"]),
    ("sample", &["skhdrc", "whkdrc", "sxhkdrc"]),
    ("scopes", &["skhdrc", "whkdrc"]),
    ("define", &["skhdrc", "whkdrc"]),
    ("blacklist", &["skhdrc"]),
    ("modes", &["skhdrc"]),
    ("pause", &["whkdrc"]),
    ("chords", &["sxhkdrc"]),
    ("release", &["sxhkdrc"]),
    ("vm", &["skhdrc", "whkdrc", "sxhkdrc"]),
];

/// The keymap of each case, as [`render`] writes it.
const EXPECTED: &[(&str, &str)] = &[
    (
        "plain",
        "Alt+H: komorebic focus left
Alt+J: komorebic focus down
Alt+K: komorebic focus up
Alt+L: komorebic focus right
Alt+Q: komorebic close",
    ),
    (
        "modifiers",
        "Meta+Return: konsole
Ctrl+Alt+Left: komorebic stack left
Alt+Shift+H: komorebic move left
Ctrl+Shift+Right: komorebic resize-edge right increase
Alt+!: komorebic move-to-workspace 0
Ctrl+Alt+(: komorebic move-to-workspace 8
Meta+Ctrl+Alt+Shift+A: komorebic retile
Ctrl+Alt+Shift+M: komorebic toggle-monocle",
    ),
    (
        "keycodes",
        "Alt+[: komorebic cycle-stack previous
Alt+]: komorebic cycle-stack next
Alt+;: komorebic unstack
Alt+=: komorebic resize-axis horizontal increase
Alt+-: komorebic resize-axis horizontal decrease
Alt+{: komorebic cycle-focus previous",
    ),
    (
        "comments",
        "Alt+H: komorebic focus left
Alt+X: komorebic flip-layout horizontal",
    ),
    (
        "braces",
        "Alt+H: komorebic focus left
Alt+J: komorebic focus down
Alt+K: komorebic focus up
Alt+L: komorebic focus right
Alt+1: komorebic focus-workspace 0
Alt+2: komorebic focus-workspace 1
Alt+3: komorebic focus-workspace 2
Alt+!: komorebic move-to-workspace 0
Alt+@: komorebic move-to-workspace 1
Alt+#: komorebic move-to-workspace 2",
    ),
    (
        "scopes",
        "Alt+N: firefox: komorebic focus-workspace 1 | kitty: ignore | *: komorebic focus-workspace 0
Alt+H: komorebic focus left",
    ),
    (
        "define",
        "Alt+B: firefox: komorebic toggle-monocle | chromium: komorebic toggle-monocle | *: ignore
Alt+H: komorebic focus left
Alt+L: komorebic focus right",
    ),
    (
        "blacklist",
        "blacklist: steam, dota2
Alt+H: komorebic focus left",
    ),
    (
        "modes",
        "Alt+R: mode resize
Alt+Q: komorebic close
mode resize, captures, on enter: notify-send resize
[resize] H: komorebic resize-edge left increase
[resize] L: komorebic resize-edge right increase
[resize] Esc: mode default
[resize] Alt+Q: komorebic close",
    ),
    (
        "pause",
        "pause Alt+Shift+P: komorebic toggle-pause
Alt+H: komorebic focus left",
    ),
    (
        "chords",
        "Alt+W, H: komorebic move left
Alt+W, L: komorebic move right
Alt+R: mode Alt+R
mode Alt+R
[Alt+R] H: komorebic resize-edge left increase
[Alt+R] L: komorebic resize-edge right increase
[Alt+R] Esc: mode default",
    ),
    ("release", "release Alt+Space: komorebic toggle-float"),
    (
        "vm",
        "Alt+J: touch /tmp/keys-corpus-alt-j
Alt+,: touch /tmp/keys-corpus-alt-comma",
    ),
];

/// Writes a keymap out, one line per binding, mode, blacklist and pause binding. Where each binding
/// was written, and the defines, are left out.
fn render(keymap: &Keymap) -> String {
    let run = |run: &Run| match run {
        Run::Command(command) => command.clone(),
        Run::Mode(mode) => format!("mode {mode}"),
    };
    let mut lines = Vec::new();
    if !keymap.blacklist.is_empty() {
        lines.push(format!("blacklist: {}", keymap.blacklist.join(", ")));
    }
    if let Some(pause) = &keymap.pause {
        lines.push(format!(
            "pause {}: {}",
            pause.combo,
            pause.hook.as_deref().unwrap_or("nothing")
        ));
    }
    for mode in &keymap.modes {
        let prefix = if mode.name == DEFAULT_MODE {
            String::new()
        } else {
            let mut header = format!("mode {}", mode.name);
            if mode.capture {
                header.push_str(", captures");
            }
            if let Some(command) = &mode.on_enter {
                header.push_str(&format!(", on enter: {command}"));
            }
            lines.push(header);
            format!("[{}] ", mode.name)
        };
        for binding in &mode.bindings {
            let chord: Vec<String> = binding.chord.iter().map(ToString::to_string).collect();
            let edge = match binding.edge {
                Edge::Press => "",
                Edge::Release => "release ",
            };
            let body = match &binding.body {
                Body::Run(action) => run(action),
                Body::Scoped(scopes) => scopes
                    .iter()
                    .map(|scope| {
                        let app = match &scope.app {
                            App::Named(name) => name.as_str(),
                            App::Other => "*",
                        };
                        let then = match &scope.then {
                            Then::Run(action) => run(action),
                            Then::Ignore => "ignore".to_string(),
                        };
                        format!("{app}: {then}")
                    })
                    .collect::<Vec<_>>()
                    .join(" | "),
            };
            lines.push(format!("{prefix}{edge}{}: {body}", chord.join(", ")));
        }
    }
    lines.join("\n")
}

/// Reads a fixture in the format its extension names.
fn parse(extension: &str, text: &str) -> Option<Parsed> {
    let format = match extension {
        "skhdrc" => Format::Skhd,
        "whkdrc" => Format::Whkd,
        "sxhkdrc" => Format::Sxhkd,
        _ => return None,
    };
    Some(komorebi_hotkeys::parse(
        format,
        text,
        &ParseOptions::default(),
    ))
}

/// The keymap with everything that says where it was written taken out: the format, the spans,
/// the bindings' source text and the defines.
fn without_provenance(mut keymap: Keymap) -> Keymap {
    keymap.format = Format::Skhd;
    keymap.defines.clear();
    if let Some(pause) = &mut keymap.pause {
        pause.span = Span::default();
    }
    for mode in &mut keymap.modes {
        for binding in &mut mode.bindings {
            binding.span = Span::default();
            binding.source.clear();
        }
    }
    keymap
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read(case: &str, extension: &str) -> String {
    let path = fixtures().join(format!("{case}.{extension}"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_corpus_is_the_files_the_table_names() {
    let mut on_disk: Vec<String> = std::fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();

    let mut named: Vec<String> = CASES
        .iter()
        .flat_map(|(case, extensions)| extensions.iter().map(move |e| format!("{case}.{e}")))
        .collect();
    named.sort();

    assert_eq!(on_disk, named);
}

#[test]
fn the_files_of_a_case_say_what_it_is_about_in_the_same_words() {
    for (case, extensions) in CASES {
        let first_lines: Vec<String> = extensions
            .iter()
            .map(|e| read(case, e).lines().next().unwrap_or_default().to_string())
            .collect();
        assert!(first_lines[0].starts_with("# "), "{case}: {first_lines:?}");
        assert!(
            first_lines.iter().all(|line| *line == first_lines[0]),
            "{case}: {first_lines:?}"
        );
    }
}

#[test]
fn every_file_reads_without_errors_into_the_keymap_of_its_case() {
    for (case, extensions) in CASES {
        for extension in *extensions {
            let Some(parsed) = parse(extension, &read(case, extension)) else {
                continue;
            };
            let errors: Vec<String> = parsed
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(ToString::to_string)
                .collect();
            assert!(errors.is_empty(), "{case}.{extension}: {errors:#?}");

            if let Some((_, expected)) = EXPECTED.iter().find(|(c, _)| c == case) {
                assert_eq!(render(&parsed.keymap), *expected, "{case}.{extension}");
            }
        }
    }
}

#[test]
fn every_case_but_the_sample_has_an_expected_keymap() {
    for (case, _) in CASES {
        let expected = EXPECTED.iter().any(|(c, _)| c == case);
        assert_eq!(expected, *case != "sample", "{case}");
    }
}

#[test]
fn the_sample_reads_completely() {
    for extension in ["skhdrc", "whkdrc", "sxhkdrc"] {
        let parsed = parse(extension, &read("sample", extension)).unwrap();
        let lossy: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|d| d.severity != Severity::Note)
            .collect();
        assert!(lossy.is_empty(), "{extension}: {lossy:#?}");
        let bindings = &parsed.keymap.modes[0].bindings;
        assert_eq!(bindings.len(), 50);

        let rendered = render(&parsed.keymap);
        for line in [
            "Alt+L: komorebic focus right",
            "Alt+{: komorebic cycle-focus previous",
            "Alt+[: komorebic cycle-stack previous",
            "Alt+(: komorebic move-to-workspace 8",
            "Alt+=: komorebic resize-axis horizontal increase",
        ] {
            assert!(rendered.lines().any(|l| l == line), "{extension}: {line}");
        }

        // no two bindings share a key
        let mut keys: Vec<_> = bindings.iter().map(|b| b.chord.clone()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), bindings.len());
    }
}

#[test]
fn the_files_of_a_case_read_into_the_same_keymap() {
    for (case, extensions) in CASES {
        let keymaps: Vec<(&str, Keymap)> = extensions
            .iter()
            .filter_map(|e| Some((*e, parse(e, &read(case, e))?.keymap)))
            .map(|(e, keymap)| (e, without_provenance(keymap)))
            .collect();
        for pair in keymaps.windows(2) {
            assert_eq!(
                pair[0].1, pair[1].1,
                "{case}: {} and {}",
                pair[0].0, pair[1].0
            );
        }
    }
}

#[test]
fn the_sample_plans_into_the_same_shortcut_files_in_every_format() {
    let mut planned = Vec::new();
    for extension in ["skhdrc", "whkdrc", "sxhkdrc"] {
        let parsed = parse(extension, &read("sample", extension)).unwrap();
        let plan = plan(
            &parsed.keymap,
            &PlanOptions {
                mode: PlanMode::Auto,
            },
        );
        assert!(!plan.needs_engine(), "{extension}");
        assert!(
            plan.diagnostics.is_empty(),
            "{extension}: {:#?}",
            plan.diagnostics
        );

        let files = desktop_files(&plan.static_files);
        assert_eq!(files.len(), 50, "{extension}");
        // everything but the comment, which quotes the file
        let files: Vec<(String, Vec<String>)> = files
            .into_iter()
            .map(|file| {
                let lines = file
                    .content
                    .lines()
                    .filter(|line| !line.starts_with("Comment="))
                    .map(str::to_string)
                    .collect();
                (file.name, lines)
            })
            .collect();
        planned.push((extension, files));
    }
    for pair in planned.windows(2) {
        assert_eq!(pair[0].1, pair[1].1, "{} and {}", pair[0].0, pair[1].0);
    }
}

#[test]
fn the_files_of_a_case_start_the_same_engine() {
    for (case, extensions) in CASES {
        let started: Vec<(&str, Vec<Effect>)> = extensions
            .iter()
            .map(|extension| {
                let parsed = parse(extension, &read(case, extension)).unwrap();
                let plan = plan(&parsed.keymap, &PlanOptions::default());
                let mut engine = Engine::new(plan.engine, EngineOptions::default());
                (*extension, engine.start())
            })
            .collect();
        assert!(!started[0].1.is_empty(), "{case}");
        for pair in started.windows(2) {
            assert_eq!(
                pair[0].1, pair[1].1,
                "{case}: {} and {}",
                pair[0].0, pair[1].0
            );
        }
    }
}
