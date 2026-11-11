//! 카드 3장의 위치. 게임 UI 는 화면 높이에 비례해 커지고 가로로는 가운데 정렬되므로,
//! 가로 값도 높이(H) 단위로 둔다. 값은 실제 선택 화면(16:9)에서 잰 카드 바깥 테두리 기준.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
}

impl Rect {
    /// 이 사각형 안의 비율 좌표(0~1)로 잘라낸 부분.
    pub fn part(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        let (w, h) = (self.r - self.l, self.b - self.t);
        Rect { l: self.l + x0 * w, t: self.t + y0 * h, r: self.l + x1 * w, b: self.t + y1 * h }
    }
}

#[derive(Clone, Copy)]
pub struct Layout {
    /// 가운데 카드 중심 x (창 폭 대비)
    pub center_x: f32,
    /// 카드 중심 간격 (창 높이 대비)
    pub spacing: f32,
    /// 카드 폭 (창 높이 대비)
    pub card_w: f32,
    /// 카드 위쪽 y (창 높이 대비)
    pub card_top: f32,
    /// 카드 높이 (창 높이 대비)
    pub card_h: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { center_x: 0.5, spacing: 0.3408, card_w: 0.2951, card_top: 0.178, card_h: 0.487 }
    }
}

impl Layout {
    pub fn card(&self, i: usize, w: f32, h: f32) -> Rect {
        let cx = self.center_x * w + (i as f32 - 1.0) * self.spacing * h;
        let half = self.card_w * h / 2.0;
        Rect { l: cx - half, t: self.card_top * h, r: cx + half, b: (self.card_top + self.card_h) * h }
    }

    pub fn hover(&self, x: f32, y: f32, w: f32, h: f32) -> Option<usize> {
        (0..3).find(|&i| {
            let c = self.card(i, w, h);
            x >= c.l && x < c.r && y >= c.t && y < c.b
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_hits_cards_not_gaps() {
        let l = Layout::default();
        let (w, h) = (1920.0, 1080.0);
        assert_eq!(l.hover(0.5 * w, 0.4 * h, w, h), Some(1));
        assert_eq!(l.hover(0.31 * w, 0.4 * h, w, h), Some(0));
        assert_eq!(l.hover(0.69 * w, 0.4 * h, w, h), Some(2));
        assert_eq!(l.hover(0.404 * w, 0.4 * h, w, h), None); // 카드 사이
        assert_eq!(l.hover(0.5 * w, 0.1 * h, w, h), None); // 카드 위
    }

    #[test]
    fn cards_stay_centered_on_wide_screens() {
        let l = Layout::default();
        let (a, b) = (l.card(1, 1920.0, 1080.0), l.card(1, 2560.0, 1080.0));
        assert_eq!(a.r - a.l, b.r - b.l);
        assert_eq!((b.l + b.r) / 2.0, 1280.0);
    }
}
