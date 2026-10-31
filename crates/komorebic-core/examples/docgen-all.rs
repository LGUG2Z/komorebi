//! Write the pages of `docs/cli` of the platforms that live in this crate, on any host:
//! `cargo run -p komorebic-core --example docgen-all -- OUTDIR [windows|macos]...`
//! writes `OUTDIR/<platform>/<command>.md` for each platform named (all of them by default).
//! The pages are the ones `komorebic docgen` writes on that platform.

use komorebic_core::cli::Meta;
use komorebic_core::cli::command;
use komorebic_core::docgen::DocFormat;
use komorebic_core::docgen::write;
use std::path::PathBuf;

const META: Meta = Meta {
    name: "komorebic",
    about: "",
    author: "",
    version: "",
};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("usage: docgen-all OUTDIR [platform]..."));
    let mut platforms: Vec<String> = args.collect();
    if platforms.is_empty() {
        platforms = ["macos"].map(String::from).to_vec();
    }
    for platform in platforms {
        let cli = match platform.as_str() {
            "macos" => command::<komorebic_core::platform::macos::MacosCommands>(&META),
            other => panic!("no platform {other} in this crate"),
        };
        let pages = write(cli, &DocFormat::PLAIN, &out.join(&platform))?;
        println!("{platform}: {} pages", pages.len());
    }
    Ok(())
}
