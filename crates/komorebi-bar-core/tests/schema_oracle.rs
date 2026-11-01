//! The schema oracle of the shared bar.
//!
//! `schemas/windows/schema.bar.json` and `schemas/macos/schema.bar.json` are what
//! `komorebi-bar --schema` prints. They are pinned here by size and a FNV-1a hash of their bytes,
//! so that a move or a convergence of bar code cannot change them unnoticed. A reviewed change
//! to either file updates the matching pin in the same revision.
//!
//! The bar binaries cannot run on Linux (Windows and AppKit APIs), so this test cannot
//! regenerate the schema until the configuration types live in this crate; from then on it
//! compares the generated text with the file instead of a hash.

use std::path::PathBuf;

struct Pin {
    file: &'static str,
    lines: usize,
    bytes: usize,
    fnv1a: u64,
}

const PINS: [Pin; 2] = [
    Pin {
        file: "schemas/windows/schema.bar.json",
        lines: 9_091,
        bytes: 248_539,
        fnv1a: 0xaaa0_fabf_b459_67b7,
    },
    Pin {
        file: "schemas/macos/schema.bar.json",
        lines: 3_774,
        bytes: 116_925,
        fnv1a: 0xa042_f6ac_063a_0cfd,
    },
];

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn read(file: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(file);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn the_checked_in_bar_schemas_are_the_pinned_ones() {
    for pin in PINS {
        let bytes = read(pin.file);
        let lines = bytes.iter().filter(|byte| **byte == b'\n').count();
        assert_eq!(lines, pin.lines, "{}: line count", pin.file);
        assert_eq!(bytes.len(), pin.bytes, "{}: size", pin.file);
        assert_eq!(
            fnv1a(&bytes),
            pin.fnv1a,
            "{}: FNV-1a (is {:#018x})",
            pin.file,
            fnv1a(&bytes)
        );
    }
}

#[test]
fn the_checked_in_bar_schemas_are_json_schemas_of_komobar_config() {
    for pin in PINS {
        let text = String::from_utf8(read(pin.file)).expect("utf-8");
        assert!(text.starts_with("{\n  \"$schema\""), "{}", pin.file);
        assert!(
            text.contains("\"title\": \"KomobarConfig\""),
            "{}",
            pin.file
        );
        assert!(text.ends_with("}\n"), "{}", pin.file);
    }
}

mod views {
    use komorebi_bar_core::schema::SchemaPlatform;
    use komorebi_bar_core::schema::view_text;

    fn text(file: &str) -> String {
        String::from_utf8(super::read(file)).expect("utf-8")
    }

    /// The Windows file is the union; its Windows view is itself.
    #[test]
    fn the_windows_view_of_the_union_is_the_windows_file() {
        let union = serde_json::from_str(&text("schemas/windows/schema.bar.json")).expect("json");
        assert!(
            view_text(union, SchemaPlatform::Windows) == text("schemas/windows/schema.bar.json")
        );
    }

    /// The macOS view of the union is the macOS file, byte for byte.
    #[test]
    fn the_macos_view_of_the_union_is_the_macos_file() {
        let union = serde_json::from_str(&text("schemas/windows/schema.bar.json")).expect("json");
        let view = view_text(union, SchemaPlatform::MacOs);
        let file = text("schemas/macos/schema.bar.json");
        if view != file {
            let first = view.lines().zip(file.lines()).position(|(a, b)| a != b);
            panic!(
                "the views differ, first at line {first:?} (view {} lines, file {})",
                view.lines().count(),
                file.lines().count()
            );
        }
    }

    /// A view applied to its own output changes nothing, so a bar can print its generated
    /// schema through it.
    #[test]
    fn a_view_is_idempotent() {
        let union = serde_json::from_str(&text("schemas/windows/schema.bar.json")).expect("json");
        let once = view_text(union, SchemaPlatform::MacOs);
        let again = view_text(
            serde_json::from_str(&once).expect("json"),
            SchemaPlatform::MacOs,
        );
        assert!(once == again);
    }
}

mod generated {
    use komorebi_bar_core::config::KomobarConfig;
    use komorebi_bar_core::schema::SchemaPlatform;
    use komorebi_bar_core::schema::generated_text;

    /// The schema the core generates from its own types, through the macOS view, is the macOS
    /// file. (Not tested for Windows: three descriptions in komorebi-protocol and the themes
    /// crate read differently when compiled for Windows, which a Linux test host cannot do; the
    /// out-of-tree harness covers it.)
    #[test]
    fn the_generated_macos_view_is_the_macos_file() {
        let file = String::from_utf8(super::read("schemas/macos/schema.bar.json")).expect("utf-8");
        let text = generated_text::<KomobarConfig>(SchemaPlatform::MacOs);
        if text != file {
            let first = text.lines().zip(file.lines()).position(|(a, b)| a != b);
            panic!(
                "differs, first at line {first:?} ({} lines, file {})",
                text.lines().count(),
                file.lines().count()
            );
        }
    }
}
