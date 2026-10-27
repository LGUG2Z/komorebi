//! The fixture corpus: key binding files that say the same thing in each format that can say it.
//!
//! Each case is a file per format in `tests/fixtures`, `<case>.skhdrc`, `<case>.whkdrc` and
//! `<case>.sxhkdrc`, for the formats that can express the case. The files of a case mean the same
//! keymap, and start with the same comment, which says what the case is about.

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
];

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
