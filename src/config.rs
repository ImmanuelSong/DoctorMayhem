//! settings.ini (exe 옆). 파일이 없으면 전부 기본값. 보정 모드에서 맞춘 카드 위치만 프로그램이 써 넣는다.
//!
//! [layout]  center_x, spacing, card_w, card_top, card_h   — src/layout.rs 참고
//! [overlay] opacity = 20~100 (%), scale = 0.5~2.0 (배지·패널 크기), panel = right | left

use crate::layout::Layout;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::WindowsProgramming::{GetPrivateProfileStringW, WritePrivateProfileStringW};

pub struct Config {
    pub layout: Layout,
    pub opacity: u8,
    pub scale: f32,
    pub panel_left: bool,
    /// 새 데이터(data.bin) 주소. settings.ini [update] url= 로 바꿀 수 있고, 비우면 업데이트 안 함.
    pub update_url: String,
}

const LAYOUT: PCWSTR = w!("layout");
const OVERLAY: PCWSTR = w!("overlay");
const UPDATE: PCWSTR = w!("update");

fn path() -> HSTRING {
    let exe = std::env::current_exe().unwrap_or_default();
    HSTRING::from(exe.with_file_name("settings.ini").as_os_str())
}

fn text(section: PCWSTR, key: PCWSTR) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetPrivateProfileStringW(section, key, w!(""), Some(&mut buf), &path()) };
    String::from_utf16_lossy(&buf[..n as usize]).trim().to_owned()
}

/// 사용자가 손으로 고치는 파일이라 범위를 벗어난 값은 잘라낸다.
fn number(section: PCWSTR, key: PCWSTR, default: f32, min: f32, max: f32) -> f32 {
    text(section, key).parse::<f32>().ok().filter(|v| v.is_finite()).map_or(default, |v| v.clamp(min, max))
}

pub fn load() -> Config {
    let d = Layout::default();
    Config {
        layout: Layout {
            center_x: number(LAYOUT, w!("center_x"), d.center_x, 0.0, 1.0),
            spacing: number(LAYOUT, w!("spacing"), d.spacing, 0.05, 1.0),
            card_w: number(LAYOUT, w!("card_w"), d.card_w, 0.05, 1.0),
            card_top: number(LAYOUT, w!("card_top"), d.card_top, 0.0, 0.9),
            card_h: number(LAYOUT, w!("card_h"), d.card_h, 0.05, 1.0),
        },
        opacity: number(OVERLAY, w!("opacity"), 100.0, 20.0, 100.0) as u8,
        scale: number(OVERLAY, w!("scale"), 1.0, 0.5, 2.0),
        panel_left: text(OVERLAY, w!("panel")).eq_ignore_ascii_case("left"),
        update_url: match text(UPDATE, w!("url")) {
            u if u.is_empty() => crate::update::DEFAULT_URL.to_owned(),
            u if u == "-" => String::new(),
            u => u,
        },
    }
}

pub fn save_layout(l: &Layout) {
    let file = path();
    for (key, value) in [
        (w!("center_x"), l.center_x),
        (w!("spacing"), l.spacing),
        (w!("card_w"), l.card_w),
        (w!("card_top"), l.card_top),
        (w!("card_h"), l.card_h),
    ] {
        unsafe {
            let _ = WritePrivateProfileStringW(LAYOUT, key, &HSTRING::from(format!("{value:.4}")), &file);
        }
    }
}
