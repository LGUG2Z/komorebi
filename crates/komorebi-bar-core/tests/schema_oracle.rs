//! The schema oracle of the shared bar.
//!
//! `schema.bar.json` (Windows, at the root) and `komorebi-for-mac/schema.bar.json` are what
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
        file: "schema.bar.json",
        lines: 9_090,
        bytes: 248_529,
        fnv1a: 0x5573_0134_1968_bff9,
    },
    Pin {
        file: "komorebi-for-mac/schema.bar.json",
        lines: 3_774,
        bytes: 116_925,
        fnv1a: 0xc99e_0294_9424_e5d7,
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
