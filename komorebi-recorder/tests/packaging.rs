//! The recorder ships in the komorebi installer (`wix/main.wxs`), so that installer has to ship every
//! binary this crate builds: `WiX` itself only runs on Windows, so this check is what a Mac or Linux
//! machine can say about the installer.

use std::path::Path;
use std::path::PathBuf;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate is a member of the workspace")
        .to_path_buf()
}

fn installer() -> String {
    std::fs::read_to_string(repository().join("wix/main.wxs")).expect("wix/main.wxs")
}

/// The binaries cargo builds for this crate: the package's own (`src/main.rs`) and one for each
/// file in `src/bin`.
fn binaries() -> Vec<String> {
    let mut names = vec![env!("CARGO_PKG_NAME").to_string()];
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin");
    if let Ok(entries) = std::fs::read_dir(bin) {
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.extension().is_some_and(|extension| extension == "rs") {
                names.push(
                    path.file_stem()
                        .expect("a file name")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    names.sort();
    names
}

#[test]
fn every_binary_the_crate_builds_is_in_the_komorebi_installer() {
    let text = installer();
    for name in binaries() {
        assert!(
            text.contains(&format!("Name='{name}.exe'")),
            "wix/main.wxs does not install {name}.exe"
        );
        assert!(
            text.contains(&format!("\\{name}.exe'")),
            "wix/main.wxs does not take {name}.exe from the build directory"
        );
    }
}
