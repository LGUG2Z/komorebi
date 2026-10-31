//! Throwaway: what does `capture` return for a window, and how long does it take?
//!
//! `cargo run -p komorebi --example capture_spike -- <window id>...`
//!
//! Ids come from `komorebic state`. Try a window that is on screen, and one that is parked.

use komorebi::animation::capture::capture;
use objc2_core_foundation::CFData;
use objc2_core_graphics::CGDataProvider;
use objc2_core_graphics::CGImage;
use std::time::Instant;

fn main() {
    for arg in std::env::args().skip(1) {
        let id: u32 = arg.parse().expect("a window id");

        for attempt in 0..3 {
            let started = Instant::now();
            let still = capture(id);
            let took = started.elapsed();

            let Some(still) = still else {
                println!("{id} #{attempt}: no picture ({took:?})");
                continue;
            };

            let provider = CGImage::data_provider(Some(&still.image));
            let data = provider.and_then(|provider| CGDataProvider::data(Some(&provider)));
            let (opaque, nonblank) = data.as_deref().map_or((0.0, 0.0), sample);

            println!(
                "{id} #{attempt}: {}x{} px, {} bytes/row, {:.1} MiB, {took:?}, alpha>0: {:.0}%, non-black: {:.0}%",
                still.width,
                still.height,
                CGImage::bytes_per_row(Some(&still.image)),
                still.bytes() as f64 / 1_048_576.0,
                opaque * 100.0,
                nonblank * 100.0,
            );
        }
    }
}

/// The share of pixels (BGRA) that have any alpha, and the share of those that are not black
fn sample(data: &CFData) -> (f64, f64) {
    let bytes = data.to_vec();
    let (mut total, mut visible, mut lit) = (0usize, 0usize, 0usize);

    for pixel in bytes.chunks_exact(4).step_by(37) {
        total += 1;
        if pixel[3] > 0 {
            visible += 1;
            if pixel[0] > 8 || pixel[1] > 8 || pixel[2] > 8 {
                lit += 1;
            }
        }
    }

    if total == 0 || visible == 0 {
        return (0.0, 0.0);
    }

    (visible as f64 / total as f64, lit as f64 / visible as f64)
}
