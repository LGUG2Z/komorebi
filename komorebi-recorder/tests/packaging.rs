//! The Windows installer (`wix/main.wxs`) has to ship every binary the crate builds and has to be
//! its own product: `WiX` itself only runs on Windows, so these checks are what a Mac or Linux
//! machine can say about the installer.

use std::path::Path;
use std::path::PathBuf;

/// komorebi's own installer: reusing its `UpgradeCode` would make the two products upgrade and
/// uninstall each other.
const KOMOREBI_UPGRADE_CODE: &str = "F8B967B5-7E7B-4E3A-895B-B789EC898B54";

/// komorebi's `PATH` component, which the installer tracks the `PATH` entry by.
const KOMOREBI_PATH_COMPONENT: &str = "6C6DF276-06C4-4675-BDED-48C5C2BC9BC5";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate is a member of the workspace")
        .to_path_buf()
}

fn installer() -> String {
    std::fs::read_to_string(repository().join("wix/main.wxs")).expect("wix/main.wxs")
}

/// The value of attribute `name` on the first element of `tag` in the installer.
fn attribute(text: &str, tag: &str, name: &str) -> String {
    let start = text
        .find(&format!("<{tag} "))
        .unwrap_or_else(|| panic!("no <{tag}> in the installer"));
    let element = &text[start..start + text[start..].find('>').expect("a closed tag")];
    let key = format!("{name}='");
    let value = element
        .find(&key)
        .unwrap_or_else(|| panic!("<{tag}> has no {name}"))
        + key.len();
    element[value..][..element[value..].find('\'').expect("a closed value")].to_string()
}

fn is_guid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.iter().map(|part| part.len()).collect::<Vec<_>>() == [8, 4, 4, 4, 12]
        && parts
            .iter()
            .all(|part| part.chars().all(|c| c.is_ascii_hexdigit()))
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
fn every_binary_the_crate_builds_is_in_the_installer() {
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

#[test]
fn the_installer_installs_nothing_the_crate_does_not_build() {
    let text = installer();
    let built = binaries();
    for piece in text.split("Name='").skip(1) {
        let name = &piece[..piece.find('\'').expect("a closed value")];
        if let Some(binary) = name.strip_suffix(".exe") {
            assert!(
                built.iter().any(|built| built == binary),
                "wix/main.wxs installs {name}, which this crate does not build"
            );
        }
    }
}

#[test]
fn the_installer_is_its_own_product() {
    let text = installer();

    let upgrade_code = attribute(&text, "Product", "UpgradeCode");
    assert!(
        is_guid(&upgrade_code),
        "UpgradeCode is not a GUID: {upgrade_code}"
    );
    assert!(
        !upgrade_code.eq_ignore_ascii_case(KOMOREBI_UPGRADE_CODE),
        "UpgradeCode is komorebi's: the two products would replace each other"
    );
    assert_eq!(attribute(&text, "Product", "Name"), env!("CARGO_PKG_NAME"));

    assert!(
        !text
            .to_uppercase()
            .contains(&KOMOREBI_PATH_COMPONENT.to_uppercase()),
        "the PATH component has komorebi's GUID"
    );
    let path_guid = attribute(&text, "Component Id='Path'", "Guid");
    assert!(
        is_guid(&path_guid),
        "the PATH component's Guid: {path_guid}"
    );
}

#[test]
fn the_license_the_installer_shows_is_there_and_is_the_projects() {
    let rtf = std::fs::read_to_string(repository().join("wix/License.rtf")).expect("License.rtf");
    assert!(
        rtf.contains("Komorebi License"),
        "not the project's license"
    );
    assert!(rtf.contains("Version 2.0.0"), "not the project's license");
    assert!(
        installer().contains("wix\\License.rtf"),
        "main.wxs does not refer to wix\\License.rtf"
    );
}
