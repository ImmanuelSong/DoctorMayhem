//! 투명·클릭 통과 최상위 창. 32bit DIB 에 Direct2D 로 그린 뒤 UpdateLayeredWindow 로 올린다.
//! 상태가 바뀔 때만 show() 를 부르고, 숨길 때 DIB 를 해제한다.

use crate::data::{self, Grade};
use crate::layout::Rect;
use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, RegisterClassW, ShowWindow, UpdateLayeredWindow, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WNDCLASSW, WNDPROC, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};
use windows_numerics::Vector2;

const CLASS: PCWSTR = w!("MayhemOverlay");
const FONT: PCWSTR = w!("맑은 고딕");

const fn rgba(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: (hex >> 16 & 0xFF) as f32 / 255.0,
        g: (hex >> 8 & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a,
    }
}
const BACKDROP: D2D1_COLOR_F = rgba(0x000000, 0.78);
const WHITE: D2D1_COLOR_F = rgba(0xFFFFFF, 1.0);
const GRADE_S: D2D1_COLOR_F = rgba(0xFF8A1E, 1.0);
const GRADE_A: D2D1_COLOR_F = rgba(0x3B9DFF, 1.0);
const UNKNOWN: D2D1_COLOR_F = rgba(0x808080, 1.0);
const DIM: D2D1_COLOR_F = rgba(0xA8B0BC, 1.0);
const CHIP: D2D1_COLOR_F = rgba(0xFFFFFF, 0.14);
const WARN: D2D1_COLOR_F = rgba(0xFFD24A, 1.0);
const BEST: D2D1_COLOR_F = rgba(0x1FB85A, 1.0);
const GUIDE: D2D1_COLOR_F = rgba(0x35E06A, 1.0);

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// 카드 한 장에 그릴 내용. Unknown = 인식 실패("?"), Ungraded = S/A 가 아님(표시 안 함).
#[derive(Clone, Copy)]
pub enum Card {
    Unknown,
    Ungraded,
    Graded(data::Entry),
}

pub struct Scene {
    /// 보정 모드: 카드 테두리 가이드와 조작법을 그린다.
    pub guide: bool,
    pub cards: [(Rect, Card); 3],
    /// BEST 카드에만 Some(따라갈 빌드 이름, 0 = 없음). 등급이 없는 카드는 띠에 빌드 이름을 쓴다.
    pub best: [Option<u32>; 3],
    /// 옆 패널: (위치 — l, t, r 만 쓰고 높이는 내용에 맞춤, 보여줄 항목)
    pub panel: Option<(Rect, Panel)>,
}

pub struct Panel {
    pub aug: u16,
    /// 이 챔피언의 통계 등급 항목(없으면 등급 없는 증강).
    pub entry: Option<data::Entry>,
    /// 이 카드를 고르면 따라갈 빌드.
    pub build: Option<data::Build>,
    /// 이미 고른 증강 중 이 증강과 같은 빌드이거나 같은 세트인 것.
    pub partners: Vec<u16>,
    /// 빌드에서 다음에 노릴 증강.
    pub next: Vec<u16>,
    /// 이번 게임에서 고른 증강 전부.
    pub picked: Vec<u16>,
}

struct Surface {
    dc: HDC,
    bits: *mut u8,
    bmp: HBITMAP,
    old: HGDIOBJ,
    w: i32,
    h: i32,
}

impl Surface {
    fn new(w: i32, h: i32) -> Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let old = SelectObject(dc, bmp.into());
            Ok(Surface { dc, bits: bits.cast(), bmp, old, w, h })
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

pub struct Overlay {
    pub hwnd: HWND,
    d2d: ID2D1Factory,
    dw: IDWriteFactory,
    // ponytail: 클라이언트 전체 크기 DIB (1080p 8MB, 4K 33MB). 메모리가 문제면 배지·패널 영역만 덮는 창으로.
    surface: Option<Surface>,
    /// 0~255, settings.ini 의 opacity
    pub alpha: u8,
    /// 배지·패널 크기 배율, settings.ini 의 scale
    pub scale: f32,
}

impl Overlay {
    pub fn new(wndproc: WNDPROC) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?.into();
            RegisterClassW(&WNDCLASSW { lpfnWndProc: wndproc, hInstance: instance, lpszClassName: CLASS, ..Default::default() });
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                w!("Mayhem Overlay"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )?;
            Ok(Overlay {
                hwnd,
                d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                dw: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
                surface: None,
                alpha: 255,
                scale: 1.0,
            })
        }
    }

    pub fn hide(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.surface = None;
    }

    fn paint(&mut self, w: i32, h: i32, scene: &Scene) -> Result<HDC> {
        if self.surface.as_ref().is_none_or(|s| (s.w, s.h) != (w, h)) {
            self.surface = None;
            self.surface = Some(Surface::new(w, h)?);
        }
        let dc = self.surface.as_ref().unwrap().dc;
        unsafe {
            // 소프트웨어 렌더: D3D 장치를 만들지 않아 메모리가 적고, 상태가 바뀔 때만 그리므로 충분.
            let rt = self.d2d.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                ..Default::default()
            })?;
            rt.BindDC(dc, &RECT { left: 0, top: 0, right: w, bottom: h })?;
            rt.BeginDraw();
            rt.Clear(Some(&rgba(0, 0.0)));
            let painter = Painter { rt: &rt, dw: &self.dw, unit: h as f32 / 1080.0 * self.scale };
            for ((rect, card), best) in scene.cards.iter().zip(scene.best) {
                painter.badge(rect, card, best)?;
            }
            if let Some((rect, panel)) = &scene.panel {
                painter.panel(rect, panel)?;
            }
            if scene.guide {
                painter.guide(&scene.cards, w as f32, h as f32)?;
            }
            rt.EndDraw(None, None)?;
        }
        Ok(dc)
    }

    /// 오프라인 확인용: 화면에 띄우지 않고 그린 결과를 32bit BMP(프리멀티플라이드 알파)로 저장.
    pub fn save_bmp(&mut self, path: &str, w: i32, h: i32, scene: &Scene) -> Result<()> {
        self.paint(w, h, scene)?;
        let len = (w * h * 4) as usize;
        let mut out = Vec::with_capacity(54 + len);
        out.extend_from_slice(b"BM");
        for v in [54 + len as u32, 0, 54, 40, w as u32, -h as u32, 1 | 32 << 16, 0, len as u32, 0, 0, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(unsafe { std::slice::from_raw_parts(self.surface.as_ref().unwrap().bits, len) });
        Ok(std::fs::write(path, out)?)
    }

    /// 화면 좌표 (x, y) 에 w×h 로 다시 그려서 표시.
    pub fn show(&mut self, x: i32, y: i32, w: i32, h: i32, scene: &Scene) -> Result<()> {
        let dc = self.paint(w, h, scene)?;
        unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&POINT { x, y }),
                Some(&SIZE { cx: w, cy: h }),
                Some(dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    SourceConstantAlpha: self.alpha,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                    ..Default::default()
                }),
                ULW_ALPHA,
            )?;
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(())
    }
}

/// unit = 1080p 기준 1픽셀. 모든 크기는 클라이언트 높이에 비례.
struct Painter<'a> {
    rt: &'a ID2D1DCRenderTarget,
    dw: &'a IDWriteFactory,
    unit: f32,
}

impl Painter<'_> {
    fn brush(&self, color: &D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        unsafe { self.rt.CreateSolidColorBrush(color, None) }
    }

    fn format(&self, size: f32, weight: DWRITE_FONT_WEIGHT) -> Result<IDWriteTextFormat> {
        unsafe {
            self.dw.CreateTextFormat(
                FONT,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size * self.unit,
                w!("ko-kr"),
            )
        }
    }

    /// 한 줄, 세로 가운데 정렬로 r 안에 그린다(넘치면 잘림).
    fn line(&self, s: &[u16], f: &IDWriteTextFormat, centered: bool, r: D2D_RECT_F, color: &D2D1_COLOR_F) -> Result<()> {
        unsafe {
            if centered {
                f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            }
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            self.rt.DrawText(s, f, &r, &self.brush(color)?, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
        Ok(())
    }

    fn grade_circle(&self, center: Vector2, radius: f32, color: &D2D1_COLOR_F, letter: char) -> Result<()> {
        unsafe {
            self.rt.FillEllipse(&D2D1_ELLIPSE { point: center, radiusX: radius, radiusY: radius }, &self.brush(color)?);
        }
        self.line(
            &[letter as u16],
            &self.format(radius / self.unit * 1.3, DWRITE_FONT_WEIGHT_BOLD)?,
            true,
            D2D_RECT_F { left: center.X - radius, top: center.Y - radius, right: center.X + radius, bottom: center.Y + radius },
            &WHITE,
        )
    }

    /// 카드 위쪽 가장자리에 붙는 띠: 등급 원 + short 한 줄 (+ 고를 카드면 BEST).
    /// 등급이 없는 BEST 카드는 따라갈 빌드 이름을 쓴다.
    fn badge(&self, card: &Rect, content: &Card, best: Option<u32>) -> Result<()> {
        let (circle, mut short) = match content {
            Card::Unknown => (Some((UNKNOWN, '?')), Vec::new()),
            Card::Graded(e) => (Some((grade_color(e.grade), grade_letter(e.grade))), data::text(e.short)),
            Card::Ungraded => (None, Vec::new()),
        };
        if let (Card::Ungraded, Some(name)) = (content, best) {
            short = if name == 0 { utf16("BEST") } else { data::text(name) };
        }
        if circle.is_none() && short.is_empty() {
            return Ok(());
        }
        let u = self.unit;
        let h = 44.0 * u;
        let bar = D2D_RECT_F { left: card.l, top: card.t - h, right: card.r, bottom: card.t };
        if !short.is_empty() {
            unsafe {
                self.rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: bar, radiusX: h / 2.0, radiusY: h / 2.0 },
                    &self.brush(&BACKDROP)?,
                );
            }
        }
        let mut text = D2D_RECT_F { left: bar.left + 16.0 * u, top: bar.top, right: bar.right - 10.0 * u, bottom: bar.bottom };
        if let Some((color, letter)) = circle {
            self.grade_circle(Vector2 { X: bar.left + h / 2.0, Y: bar.top + h / 2.0 }, 17.0 * u, &color, letter)?;
            text.left = bar.left + h + 2.0 * u;
        }
        if best.is_some() && !short.is_empty() {
            let pill = D2D_RECT_F { left: bar.right - 70.0 * u, top: bar.top + 8.0 * u, right: bar.right - 8.0 * u, bottom: bar.bottom - 8.0 * u };
            unsafe {
                self.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: pill, radiusX: 14.0 * u, radiusY: 14.0 * u }, &self.brush(&BEST)?);
            }
            self.line(&utf16("BEST"), &self.format(16.0, DWRITE_FONT_WEIGHT_BOLD)?, true, pill, &WHITE)?;
            text.right = pill.left - 6.0 * u;
        }
        self.line(&short, &self.format(19.0, DWRITE_FONT_WEIGHT_NORMAL)?, false, text, &WHITE)
    }

    /// 보정 모드: 게임의 카드 바깥 테두리에 맞출 가이드 선 + 조작법.
    fn guide(&self, cards: &[(Rect, Card); 3], w: f32, h: f32) -> Result<()> {
        let u = self.unit;
        let line = self.brush(&GUIDE)?;
        for (c, _) in cards {
            unsafe {
                self.rt.DrawRectangle(&D2D_RECT_F { left: c.l, top: c.t, right: c.r, bottom: c.b }, &line, 2.0 * u, None);
            }
        }
        let help = utf16(
            "보정 모드 — 초록 선을 카드 바깥 테두리에 맞추세요\n\
             방향키: 이동   Shift+방향키: 카드 크기   Ctrl+←→: 카드 간격\n\
             Enter: 저장하고 끝내기   Ctrl+Alt+F9: 저장하지 않고 끝내기",
        );
        let (bw, bh) = (620.0 * u, 96.0 * u);
        let bottom = cards[0].0.b + 0.02 * h + bh;
        let r = D2D_RECT_F { left: (w - bw) / 2.0, top: bottom - bh, right: (w + bw) / 2.0, bottom };
        unsafe {
            self.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 10.0 * u, radiusY: 10.0 * u }, &self.brush(&BACKDROP)?);
        }
        let f = self.format(17.0, DWRITE_FONT_WEIGHT_NORMAL)?;
        unsafe {
            f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            self.rt.DrawText(&help, &f, &r, &self.brush(&WHITE)?, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
        Ok(())
    }

    /// 옆 패널. 내용을 먼저 배치해 높이를 구한 뒤 바탕 → 내용 순으로 그린다.
    fn panel(&self, at: &Rect, panel: &Panel) -> Result<()> {
        let u = self.unit;
        let (pad, gap) = (16.0 * u, 12.0 * u);
        let (left, width) = (at.l + pad, at.r - at.l - 2.0 * pad);
        let mut flow = Flow { p: self, left, width, y: at.t + pad, texts: Vec::new(), chips: Vec::new() };

        let title_h = 36.0 * u;
        let circle_w = if panel.entry.is_some() { title_h + 8.0 * u } else { 0.0 };
        let title = flow.measure(&data::text(data::aug_name(panel.aug)), 22.0, DWRITE_FONT_WEIGHT_BOLD, width - circle_w)?;
        let title_y = flow.y + (title_h - title.1.height).max(0.0) / 2.0;
        flow.texts.push((title.0, left + circle_w, title_y, WHITE));
        let circle = Vector2 { X: left + title_h / 2.0, Y: flow.y + title_h / 2.0 };
        flow.y += title_h.max(title.1.height) + gap;

        if let Some(e) = &panel.entry {
            flow.paragraph(&data::text(e.why), 17.0, WHITE)?;
        }
        if let Some(b) = &panel.build {
            flow.y += 8.0 * u;
            let mut label = utf16("빌드 · ");
            label.extend(data::text(b.name));
            flow.paragraph(&label, 14.0, BEST)?;
            flow.paragraph(&data::text(b.why), 16.0, WHITE)?;
        }
        flow.chips("BEST · 고른 증강과 궁합", panel.partners.iter().map(|&a| data::aug_name(a)))?;
        flow.chips("다음에 노릴 증강", panel.next.iter().map(|&a| data::aug_name(a)))?;
        match (&panel.build, &panel.entry) {
            (Some(b), _) => flow.chips("추천 아이템", b.items.iter().map(|&i| data::item_name(i)))?,
            (None, Some(e)) => {
                let pairs_label = if e.flags & data::ENT_COMBO != 0 { "같이 먹으면 좋은 증강" } else { "이 챔피언의 다른 S 증강" };
                flow.chips(pairs_label, e.pairs.iter().filter(|&&a| a != data::NONE).map(|&a| data::aug_name(a)))?;
                flow.chips("자주 가는 아이템", e.items.iter().filter(|&&i| i != data::NONE).map(|&i| data::item_name(i)))?;
            }
            _ => {}
        }
        flow.chips("고른 증강", panel.picked.iter().map(|&a| data::aug_name(a)))?;
        if let Some(e) = panel.entry.filter(|e| e.tip != 0) {
            flow.y += gap;
            flow.paragraph(&utf16("운영 팁"), 14.0, DIM)?;
            flow.paragraph(&data::text(e.tip), 16.0, WHITE)?;
        }
        if panel.entry.is_some_and(|e| e.flags & data::ENT_VERIFY != 0) {
            flow.y += gap;
            flow.paragraph(&utf16("검증 필요"), 14.0, WARN)?;
        }
        if panel.build.is_some() {
            flow.y += gap;
            flow.paragraph(&utf16("빌드 추천: 스킬·증강 설명을 보고 만든 분석(통계 아님)"), 11.0, DIM)?;
        }
        flow.y += gap;
        flow.paragraph(&data::text(data::credit()), 11.0, DIM)?;

        let Flow { y, texts, chips, .. } = flow;
        unsafe {
            let round = |rect, r| D2D1_ROUNDED_RECT { rect, radiusX: r, radiusY: r };
            let bg = D2D_RECT_F { left: at.l, top: at.t, right: at.r, bottom: y + pad };
            self.rt.FillRoundedRectangle(&round(bg, 10.0 * u), &self.brush(&BACKDROP)?);
            let chip_brush = self.brush(&CHIP)?;
            for rect in chips {
                self.rt.FillRoundedRectangle(&round(rect, (rect.bottom - rect.top) / 2.0), &chip_brush);
            }
            for (layout, x, y, color) in texts {
                self.rt.DrawTextLayout(Vector2 { X: x, Y: y }, &layout, &self.brush(&color)?, D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }
        match &panel.entry {
            Some(e) => self.grade_circle(circle, 16.0 * u, &grade_color(e.grade), grade_letter(e.grade)),
            None => Ok(()),
        }
    }
}

fn grade_color(g: Grade) -> D2D1_COLOR_F {
    if g == Grade::S { GRADE_S } else { GRADE_A }
}
fn grade_letter(g: Grade) -> char {
    if g == Grade::S { 'S' } else { 'A' }
}

/// 패널 내용의 세로 배치. 그릴 것을 모아 두기만 한다.
struct Flow<'a> {
    p: &'a Painter<'a>,
    left: f32,
    width: f32,
    y: f32,
    texts: Vec<(IDWriteTextLayout, f32, f32, D2D1_COLOR_F)>,
    chips: Vec<D2D_RECT_F>,
}

impl Flow<'_> {
    fn measure(&self, s: &[u16], size: f32, weight: DWRITE_FONT_WEIGHT, max_w: f32) -> Result<(IDWriteTextLayout, DWRITE_TEXT_METRICS)> {
        unsafe {
            let layout = self.p.dw.CreateTextLayout(s, &self.p.format(size, weight)?, max_w, 4096.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            Ok((layout, m))
        }
    }

    fn paragraph(&mut self, s: &[u16], size: f32, color: D2D1_COLOR_F) -> Result<()> {
        if s.is_empty() {
            return Ok(());
        }
        let (layout, m) = self.measure(s, size, DWRITE_FONT_WEIGHT_NORMAL, self.width)?;
        self.texts.push((layout, self.left, self.y, color));
        self.y += m.height + 4.0 * self.p.unit;
        Ok(())
    }

    /// 라벨 한 줄 + 칩들(폭이 넘치면 다음 줄). 칩이 없으면 아무것도 안 그린다.
    fn chips(&mut self, label: &str, names: impl Iterator<Item = u32>) -> Result<()> {
        let u = self.p.unit;
        let (pad_x, pad_y, gap) = (10.0 * u, 4.0 * u, 6.0 * u);
        let (mut x, mut row_h, mut first) = (self.left, 0.0f32, true);
        for name in names {
            if first {
                self.y += 12.0 * u;
                self.paragraph(&utf16(label), 14.0, DIM)?;
                first = false;
            }
            let (layout, m) = self.measure(&data::text(name), 15.0, DWRITE_FONT_WEIGHT_NORMAL, self.width - 2.0 * pad_x)?;
            let (w, h) = (m.width + 2.0 * pad_x, m.height + 2.0 * pad_y);
            if x > self.left && x + w > self.left + self.width {
                x = self.left;
                self.y += row_h + gap;
            }
            self.chips.push(D2D_RECT_F { left: x, top: self.y, right: x + w, bottom: self.y + h });
            self.texts.push((layout, x + pad_x, self.y + pad_y, WHITE));
            x += w + gap;
            row_h = h;
        }
        self.y += row_h;
        Ok(())
    }
}
