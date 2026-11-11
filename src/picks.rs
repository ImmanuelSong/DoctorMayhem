//! 이번 게임에서 고른 증강 기록. 메모리를 읽지 않으므로 화면과 클릭으로 추정한다.
//! - 선택 화면에서 마지막으로 클릭한 카드가, 화면이 닫힐 때 아직 화면에 있으면 고른 것으로 친다(리롤된 카드는 제외).
//! - 기록한 증강이 나중 선택 화면에 다시 나오면 안 고른 것이다(고른 증강은 다시 안 나온다고 가정). 단, 방금 기록한
//!   증강이 SETTLE 안에 보이는 건 닫히는 중인 같은 화면이라 무시.
//! - undo: 마지막 기록 지우기(Ctrl+Alt+F8).

use std::time::{Duration, Instant};

const SETTLE: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct Picks {
    pub list: Vec<u16>,
    clicked: Option<u16>,
    last: Option<(u16, Instant)>,
}

impl Picks {
    /// 개발용 --picked: 이미 고른 것으로 시작.
    pub fn with(list: Vec<u16>) -> Self {
        Picks { list, ..Default::default() }
    }

    /// 선택 화면이 새로 열림.
    pub fn opened(&mut self) {
        self.clicked = None;
    }

    /// 카드를 클릭함(None = 읽지 못한 카드).
    pub fn click(&mut self, aug: Option<u16>) {
        self.clicked = aug;
    }

    /// 선택 화면이 닫힘. on_screen = 닫히기 직전 카드들.
    pub fn closed(&mut self, on_screen: &[u16], now: Instant) {
        if let Some(aug) = self.clicked.take().filter(|a| on_screen.contains(a) && !self.list.contains(a)) {
            self.list.push(aug);
            self.last = Some((aug, now));
        }
    }

    /// 선택 화면에 이 증강이 나옴.
    pub fn offered(&mut self, aug: u16, now: Instant) {
        if !self.last.is_some_and(|(a, at)| a == aug && now.duration_since(at) < SETTLE) {
            self.list.retain(|&p| p != aug);
        }
    }

    pub fn undo(&mut self) {
        self.list.pop();
    }

    /// 게임이 끝남.
    pub fn clear(&mut self) {
        *self = Picks::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_follow_clicks_and_correct_themselves() {
        let t = Instant::now();
        let mut p = Picks::default();
        p.opened();
        p.click(Some(1));
        p.closed(&[1, 2, 3], t);
        assert_eq!(p.list, [1]);
        p.offered(1, t + Duration::from_millis(300)); // 닫히는 애니메이션 중 같은 카드
        assert_eq!(p.list, [1]);

        p.opened();
        p.click(Some(5)); // 5 를 클릭했는데 리롤돼서 사라짐
        p.closed(&[4, 6, 7], t + Duration::from_secs(60));
        assert_eq!(p.list, [1]);

        p.opened();
        p.closed(&[8, 9, 10], t + Duration::from_secs(90)); // 클릭 없이 닫힘(상점 범위를 벗어남 등)
        assert_eq!(p.list, [1]);

        p.offered(1, t + Duration::from_secs(120)); // 기록한 1 이 다시 나옴 → 안 고른 것
        assert!(p.list.is_empty());

        p.opened();
        p.click(Some(2));
        p.closed(&[2, 3, 4], t + Duration::from_secs(130));
        p.undo();
        assert!(p.list.is_empty());
    }
}
