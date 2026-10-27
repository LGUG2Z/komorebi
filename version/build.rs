use std::fs::File;
use std::io::Write;

use shadow_rs::SdResult;
use shadow_rs::ShadowBuilder;

fn main() {
    let shadow_build = ShadowBuilder::builder().hook(raw_hook).build().unwrap();

    let is_flake_build = shadow_build
        .map
        .get("BRANCH")
        .map_or_else(|| true, |entry| entry.v.is_empty());

    if is_flake_build {
        ShadowBuilder::builder().hook(flake_hook).build().unwrap();
    }
}

const RAW_VERSION_CONST: &str = r##"pub const LONG_VERSION:&str = shadow_rs::formatcp!(r#"{}
branch:{}
commit_hash:{}
build_env:{},{}"#,PKG_VERSION, BRANCH, COMMIT_HASH, RUST_VERSION, RUST_CHANNEL
);
"##;

fn raw_hook(mut file: &File) -> SdResult<()> {
    writeln!(file, "{RAW_VERSION_CONST}")?;
    Ok(())
}

const FLAKE_VERSION_CONST: &str = r##"pub const LONG_VERSION:&str = shadow_rs::formatcp!(r#"{}
commit_hash:{}
build_env:{},{}"#,PKG_VERSION, env!("COMMIT_HASH"), RUST_VERSION, RUST_CHANNEL
);"##;

fn flake_hook(mut file: &File) -> SdResult<()> {
    writeln!(file, "{FLAKE_VERSION_CONST}")?;
    Ok(())
}
