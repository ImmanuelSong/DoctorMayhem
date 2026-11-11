//! 화면에서 읽은 픽셀로 판단하는 부분(순수 계산): 선택 화면 감지, 카드 변경 감지용 해시.
//! 값은 실제 선택 화면 캡처에서 잰 것. 화면을 읽기만 한다.

use crate::layout::{Layout, Rect};

/// BGRA, 위에서 아래로.
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Image {
    /// 가로세로 2배(최근접). 작은 제목 글자를 OCR 이 놓칠 때 다시 읽는 용도.
    pub fn doubled(&self) -> Image {
        let w = self.w * 2;
        let px = (0..w * self.h * 2)
            .flat_map(|i| {
                let o = ((i / w / 2) * self.w + (i % w) / 2) * 4;
                self.px[o..o + 4].to_vec()
            })
            .collect();
        Image { w, h: self.h * 2, px }
    }
}

impl Image {
    pub fn rgb(&self, x: usize, y: usize) -> [i32; 3] {
        let o = (y.min(self.h - 1) * self.w + x.min(self.w - 1)) * 4;
        [self.px[o + 2] as i32, self.px[o + 1] as i32, self.px[o] as i32]
    }
}

/// 카드 안쪽 바탕색(어두운 청록). 등급·호버와 무관하게 거의 일정하다.
const INTERIOR: [i32; 3] = [4, 18, 19];
/// 보정값: 채널별 허용 오차. 실측은 12 이내(호버로 밝아진 카드 포함), 상점·점수판은 여러 점에서 33 이상.
pub const INTERIOR_TOLERANCE: i32 = 24;
/// 보정값: 카드 위쪽 테두리의 밝은 띠(R+G+B). 실측 은·금·프리즘 모두 670 이상.
pub const HIGHLIGHT_MIN: i32 = 540;

/// 카드 안에서 항상 비어 있는 자리(카드 폭·높이 대비).
const INTERIOR_X: [f32; 2] = [0.152, 0.848];
const INTERIOR_Y: [f32; 3] = [0.25, 0.5, 0.85];
const HIGHLIGHT_X: f32 = 0.27;

/// 카드 3장이 모두 떠 있는가. `grab` 은 클라이언트 좌표 영역의 픽셀을 돌려준다.
/// 전체 화면이 아니라 1픽셀 두께의 가로줄 3개 + 짧은 세로줄 3개만 읽는다.
/// (오버레이가 그리는 배지·패널 영역과는 겹치지 않는다.)
pub fn selecting(layout: &Layout, w: f32, h: f32, grab: &mut dyn FnMut(&Rect) -> Option<Image>) -> bool {
    let cards = [0, 1, 2].map(|i| layout.card(i, w, h));
    for fy in INTERIOR_Y {
        let y = cards[0].t + fy * (cards[0].b - cards[0].t);
        let Some(row) = grab(&Rect { l: cards[0].l, t: y, r: cards[2].r, b: y + 1.0 }) else { return false };
        for c in &cards {
            for fx in INTERIOR_X {
                let p = row.rgb((c.l + fx * (c.r - c.l) - cards[0].l) as usize, 0);
                if (0..3).any(|k| (p[k] - INTERIOR[k]).abs() > INTERIOR_TOLERANCE) {
                    return false;
                }
            }
        }
    }
    cards.iter().all(|c| {
        let x = c.l + HIGHLIGHT_X * (c.r - c.l);
        grab(&Rect { l: x, t: c.t + 0.002 * h, r: x + 1.0, b: c.t + 0.022 * h })
            .is_some_and(|col| (0..col.h).any(|y| col.rgb(0, y).iter().sum::<i32>() >= HIGHLIGHT_MIN))
    })
}

/// 카드에서 읽을 영역(카드 대비 비율).
pub fn icon_area(card: &Rect) -> Rect {
    card.part(0.28, 0.09, 0.72, 0.355)
}
/// 제목 한두 줄 + 태그. OCR 은 줄 단위로 맞춰 보므로 넉넉하게 잡는다.
pub fn title_area(card: &Rect) -> Rect {
    card.part(0.10, 0.40, 0.90, 0.55)
}

/// 64bit dHash: 9x8 회색으로 줄여 가로로 이웃한 칸의 밝기를 비교.
pub fn dhash(img: &Image) -> u64 {
    let mut cells = [[0u32; 9]; 8];
    for (cy, row) in cells.iter_mut().enumerate() {
        for (cx, cell) in row.iter_mut().enumerate() {
            let (x0, x1) = (cx * img.w / 9, ((cx + 1) * img.w / 9).max(cx * img.w / 9 + 1));
            let (y0, y1) = (cy * img.h / 8, ((cy + 1) * img.h / 8).max(cy * img.h / 8 + 1));
            let mut sum = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += img.rgb(x, y).iter().sum::<i32>() as u32;
                }
            }
            *cell = sum / ((x1 - x0) * (y1 - y0)) as u32;
        }
    }
    cells.iter().flatten().enumerate().filter(|(i, _)| i % 9 != 8).fold(0u64, |h, (i, &v)| {
        h << 1 | (v > cells[i / 9][i % 9 + 1]) as u64
    })
}

/// 카드 한 장의 지문(아이콘 + 제목). 리롤로 카드가 바뀌었는지 판단하는 데만 쓴다.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Signature(pub u64, pub u64);

impl Signature {
    /// 보정값: 해밍 거리 6 이하면 같은 카드로 본다(압축·반짝임 잡음 허용).
    pub fn same(&self, other: &Signature) -> bool {
        (self.0 ^ other.0).count_ones() <= 6 && (self.1 ^ other.1).count_ones() <= 6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(img: &mut Image, r: &Rect, rgb: [u8; 3]) {
        for y in r.t as usize..(r.b as usize).min(img.h) {
            for x in r.l as usize..(r.r as usize).min(img.w) {
                img.px[(y * img.w + x) * 4..][..3].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
            }
        }
    }

    fn crop(img: &Image, r: &Rect) -> Option<Image> {
        let (x, y) = (r.l as usize, r.t as usize);
        let (w, h) = (((r.r - r.l) as usize).max(1), ((r.b - r.t) as usize).max(1));
        let px = (y..y + h).flat_map(|yy| img.px[(yy * img.w + x) * 4..(yy * img.w + x + w) * 4].to_vec()).collect();
        Some(Image { w, h, px })
    }

    #[test]
    fn detects_three_cards_only() {
        let (w, h) = (1280.0, 720.0);
        let layout = Layout::default();
        let mut screen = Image { w: 1280, h: 720, px: vec![90; 1280 * 720 * 4] };
        assert!(!selecting(&layout, w, h, &mut |r| crop(&screen, r)));
        for i in 0..3 {
            let card = layout.card(i, w, h);
            fill(&mut screen, &card, [230, 225, 215]);
            fill(&mut screen, &card.part(0.09, 0.04, 0.91, 0.97), [3, 17, 18]);
            // 카드 2장만으로는 선택 화면이 아니다
            assert_eq!(selecting(&layout, w, h, &mut |r| crop(&screen, r)), i == 2);
        }
        // 상점처럼 색은 비슷하지만 테두리 띠가 없는 화면
        let dark = Image { w: 1280, h: 720, px: [19u8, 18, 4, 255].repeat(1280 * 720) };
        assert!(!selecting(&layout, w, h, &mut |r| crop(&dark, r)));
    }

    #[test]
    fn dhash_tells_patterns_apart() {
        let mut a = Image { w: 90, h: 80, px: vec![0; 90 * 80 * 4] };
        let mut b = Image { w: 90, h: 80, px: vec![0; 90 * 80 * 4] };
        fill(&mut a, &Rect { l: 0.0, t: 0.0, r: 45.0, b: 80.0 }, [255, 255, 255]);
        fill(&mut b, &Rect { l: 20.0, t: 0.0, r: 30.0, b: 80.0 }, [255, 255, 255]);
        fill(&mut b, &Rect { l: 60.0, t: 0.0, r: 70.0, b: 80.0 }, [255, 255, 255]);
        let (ha, hb) = (dhash(&a), dhash(&b));
        assert!((ha ^ hb).count_ones() > 6);
        assert!(Signature(ha, hb).same(&Signature(ha ^ 0b111, hb)));
        assert!(!Signature(ha, hb).same(&Signature(hb, hb)));
    }
}
