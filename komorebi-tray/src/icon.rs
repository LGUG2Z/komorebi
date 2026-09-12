use crate::state::IconKind;
use color_eyre::eyre::Result;
use color_eyre::eyre::ensure;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::ANTIALIASED_QUALITY;
use windows::Win32::Graphics::Gdi::BI_RGB;
use windows::Win32::Graphics::Gdi::BITMAPINFO;
use windows::Win32::Graphics::Gdi::BITMAPINFOHEADER;
use windows::Win32::Graphics::Gdi::CLIP_DEFAULT_PRECIS;
use windows::Win32::Graphics::Gdi::CreateBitmap;
use windows::Win32::Graphics::Gdi::CreateCompatibleDC;
use windows::Win32::Graphics::Gdi::CreateDIBSection;
use windows::Win32::Graphics::Gdi::CreateFontW;
use windows::Win32::Graphics::Gdi::DEFAULT_CHARSET;
use windows::Win32::Graphics::Gdi::DEFAULT_PITCH;
use windows::Win32::Graphics::Gdi::DIB_RGB_COLORS;
use windows::Win32::Graphics::Gdi::DT_CENTER;
use windows::Win32::Graphics::Gdi::DT_NOPREFIX;
use windows::Win32::Graphics::Gdi::DT_SINGLELINE;
use windows::Win32::Graphics::Gdi::DT_VCENTER;
use windows::Win32::Graphics::Gdi::DeleteDC;
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::Graphics::Gdi::DrawTextW;
use windows::Win32::Graphics::Gdi::FF_SWISS;
use windows::Win32::Graphics::Gdi::FW_BOLD;
use windows::Win32::Graphics::Gdi::GdiFlush;
use windows::Win32::Graphics::Gdi::GetTextExtentPoint32W;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Gdi::HGDIOBJ;
use windows::Win32::Graphics::Gdi::OUT_DEFAULT_PRECIS;
use windows::Win32::Graphics::Gdi::SelectObject;
use windows::Win32::Graphics::Gdi::SetBkMode;
use windows::Win32::Graphics::Gdi::SetTextColor;
use windows::Win32::Graphics::Gdi::TRANSPARENT;
use windows::Win32::UI::WindowsAndMessaging::CreateIconIndirect;
use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::Win32::UI::WindowsAndMessaging::ICONINFO;
use windows::core::w;

pub struct Icon {
    pub handle: HICON,
    pub size: i32,
}

impl Icon {
    pub fn render(kind: IconKind, size: i32) -> Result<Self> {
        ensure!((1..=256).contains(&size), "Invalid tray icon size: {size}");
        // All GDI objects are owned on this thread and selections are restored before deletion.
        unsafe {
            let dc = MemoryDc(CreateCompatibleDC(None));
            ensure!(
                !dc.0.is_invalid(),
                "Could not create an icon device context"
            );
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc.0), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let _bitmap = GdiObject(bitmap.into());
            ensure!(!bits.is_null(), "Could not allocate icon pixels");
            {
                let pixels =
                    std::slice::from_raw_parts_mut(bits.cast::<u32>(), (size * size) as usize);
                for y in 0..size {
                    for x in 0..size {
                        // A fine light border keeps the dark badge visible on dark taskbars.
                        let border = x == 0 || y == 0 || x == size - 1 || y == size - 1;
                        pixels[(y * size + x) as usize] =
                            if border { 0xff909090 } else { 0xff202020 };
                    }
                }
            }
            if kind == IconKind::Paused {
                let pixels =
                    std::slice::from_raw_parts_mut(bits.cast::<u32>(), (size * size) as usize);
                draw_paused(pixels, size);
            } else {
                let label = match kind {
                    IconKind::Workspace(number) => number.to_string(),
                    IconKind::Unavailable => "—".to_owned(),
                    IconKind::Paused => unreachable!(),
                };
                let _selection = Selection::new(dc.0, bitmap.into())?;
                SetBkMode(dc.0, TRANSPARENT);
                SetTextColor(dc.0, COLORREF(0x00ffffff));
                let mut text: Vec<u16> = label.encode_utf16().collect();
                for height in (1..=size * 7 / 8).rev() {
                    let font = CreateFontW(
                        -height,
                        0,
                        0,
                        0,
                        FW_BOLD.0 as i32,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET,
                        OUT_DEFAULT_PRECIS,
                        CLIP_DEFAULT_PRECIS,
                        ANTIALIASED_QUALITY,
                        (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
                        w!("Segoe UI"),
                    );
                    ensure!(!font.is_invalid(), "Could not create the tray font");
                    let _font = GdiObject(font.into());
                    let _selection = Selection::new(dc.0, font.into())?;
                    let mut extent = SIZE::default();
                    GetTextExtentPoint32W(dc.0, &text, &mut extent).ok()?;
                    if (extent.cx <= size - 2 && extent.cy <= size - 2) || height == 1 {
                        let mut bounds = RECT {
                            left: 1,
                            top: 1,
                            right: size - 1,
                            bottom: size - 1,
                        };
                        ensure!(
                            DrawTextW(
                                dc.0,
                                &mut text,
                                &mut bounds,
                                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX
                            ) != 0,
                            "Could not draw the workspace number"
                        );
                        break;
                    }
                }
                GdiFlush().ok()?;
            }
            // GDI text rendering clears alpha. The badge is deliberately opaque.
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (size * size) as usize);
            for pixel in pixels {
                *pixel |= 0xff000000;
            }
            let mask_bits = vec![0u8; (((size + 15) / 16) * 2 * size) as usize];
            let mask = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr().cast()));
            ensure!(!mask.is_invalid(), "Could not create the icon mask");
            let _mask = GdiObject(mask.into());
            let handle = CreateIconIndirect(&ICONINFO {
                fIcon: true.into(),
                hbmMask: mask,
                hbmColor: bitmap,
                ..Default::default()
            })?;
            Ok(Self { handle, size })
        }
    }
}

// Cross the existing badge from corner to corner without drawing another frame.
fn draw_paused(pixels: &mut [u32], size: i32) {
    let half_stroke = (size as f32 / 16.0).max(1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let xf = x as f32;
            let yf = y as f32;
            let cross = (xf - yf).abs() <= half_stroke * std::f32::consts::SQRT_2
                || (xf + yf - (size - 1) as f32).abs() <= half_stroke * std::f32::consts::SQRT_2;
            if cross {
                pixels[(y * size + x) as usize] = 0xffffffff;
            }
        }
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.handle);
        }
    }
}

struct MemoryDc(HDC);

impl Drop for MemoryDc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct GdiObject(HGDIOBJ);

impl Drop for GdiObject {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0);
        }
    }
}

struct Selection {
    dc: HDC,
    previous: HGDIOBJ,
}

impl Selection {
    fn new(dc: HDC, object: HGDIOBJ) -> Result<Self> {
        let previous = unsafe { SelectObject(dc, object) };
        ensure!(
            !previous.is_invalid(),
            "Could not select an icon drawing object"
        );
        Ok(Self { dc, previous })
    }
}

impl Drop for Selection {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::BITMAP;
    use windows::Win32::Graphics::Gdi::GetObjectW;
    use windows::Win32::System::Threading::GR_GDIOBJECTS;
    use windows::Win32::System::Threading::GetCurrentProcess;
    use windows::Win32::System::Threading::GetGuiResources;
    use windows::Win32::UI::WindowsAndMessaging::GetIconInfo;

    #[test]
    fn renders_multiple_digits_and_scales_without_leaking_gdi_objects() {
        drop(Icon::render(IconKind::Workspace(1), 16).unwrap());
        let before = unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) };
        for _ in 0..10 {
            for size in [16, 24, 32] {
                for kind in [
                    IconKind::Workspace(1),
                    IconKind::Workspace(12),
                    IconKind::Workspace(100),
                    IconKind::Unavailable,
                    IconKind::Paused,
                ] {
                    let icon = Icon::render(kind, size).unwrap();
                    unsafe {
                        let mut info = ICONINFO::default();
                        GetIconInfo(icon.handle, &mut info).unwrap();
                        let _color = GdiObject(info.hbmColor.into());
                        let _mask = GdiObject(info.hbmMask.into());
                        let mut bitmap = BITMAP::default();
                        assert_ne!(
                            GetObjectW(
                                info.hbmColor.into(),
                                size_of::<BITMAP>() as i32,
                                Some((&mut bitmap as *mut BITMAP).cast())
                            ),
                            0
                        );
                        assert_eq!((bitmap.bmWidth, bitmap.bmHeight), (size, size));
                    }
                }
            }
        }
        let after = unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) };
        assert_eq!(before, after, "GDI objects leaked while replacing icons");
    }
}
