use color_eyre::Result;
use komorebi_core::Rect;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

pub struct Animation;

impl Animation {
    pub fn lerp(x: i32, new_x: i32, t: f64) -> i32 {
        (x as f64 + (new_x - x) as f64 * t) as i32
    }

    pub fn lerp_rect(original_rect: &Rect, new_rect: &Rect, t: f64) -> Rect {
        let is_half_way = t > 0.5;
        let mut rect = Rect::default();
        rect.top = Animation::lerp(original_rect.top, new_rect.top, t);
        rect.left = Animation::lerp(original_rect.left, new_rect.left, t);
        rect.bottom = if is_half_way {
            new_rect.bottom
        } else {
            original_rect.bottom
        };
        rect.right = if is_half_way {
            new_rect.right
        } else {
            original_rect.right
        };

        rect
    }

    pub fn animate(duration: Duration, mut f: impl FnMut(f64) -> Result<()>) -> bool {
        let target_frame_time = Duration::from_millis(1000 / 240);
        let mut progress = 0.0;
        let &animation_start = &Instant::now();

        while progress < 1.0 {
            let tick_start = Instant::now();
            f(progress).unwrap();
            progress = animation_start.elapsed().as_millis() as f64 / duration.as_millis() as f64;

            if progress > 1.0 {
                progress = 1.0;
            }

            while tick_start.elapsed() < target_frame_time {
                sleep(target_frame_time - tick_start.elapsed());
            }
        }

        f(progress).unwrap();
        true
    }
}
