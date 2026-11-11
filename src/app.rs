//! 진입점: 타이머 하나로 도는 상태 머신 + 단축키.
//! IDLE(게임 창 없음, 2초) → IN_GAME(선택 화면 감시 300ms) → SELECTING(카드 감시 100ms, 호버 33ms)

use crate::capture::{self, Ocr, Source};
use crate::config;
use crate::data;
use crate::detect::{self, Image, Signature};
use crate::layout::{Layout, Rect};
use crate::live_client;
use crate::update;
use crate::picks::Picks;
use crate::overlay::{Card, Overlay, Panel, Scene};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows::core::{w, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_DOWN, VK_F10, VK_F8,
    VK_F9, VK_LBUTTON, VK_LEFT, VK_OEM_3, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetClientRect, GetCursorPos, GetForegroundWindow,
    GetMessageW, IsIconic, PostQuitMessage, SetTimer, MSG, WM_DESTROY, WM_HOTKEY, WM_TIMER,
};

const GAME_CLASS: PCWSTR = w!("RiotWindowClass");
const GAME_TITLE: PCWSTR = w!("League of Legends (TM) Client");
const TIMER: usize = 1;
const IDLE_MS: u32 = 2000;
const IN_GAME_MS: u32 = 300;
const HOVER_MS: u32 = 33;
/// 선택 화면이 열려 있는 동안의 감시 주기(닫힘·리롤을 빨리 알아채려고 더 자주 본다).
const SELECTING_WATCH: Duration = Duration::from_millis(100);
const IN_GAME_WATCH: Duration = Duration::from_millis(IN_GAME_MS as u64);
/// 깜빡임 방지: 연속 2회 같은 판정이 나와야 열림/닫힘으로 본다.
const CONFIRMATIONS: u8 = 2;
const CHAMPION_RETRY: Duration = Duration::from_secs(5);
const HK_TOGGLE: i32 = 1;
const HK_QUIT: i32 = 2;
const HK_CALIBRATE: i32 = 3;
const HK_SAVE: i32 = 4;
const HK_UNDO_PICK: i32 = 5;
/// 보정 모드 방향키: HK_ARROWS + 변경키(0 없음, 1 Shift, 2 Ctrl) * 4 + 방향(←→↑↓)
const HK_ARROWS: i32 = 10;
/// 보정 한 번에 움직이는 양(1080p 에서 약 1픽셀).
const CALIBRATE_STEP: f32 = 0.001;

/// 게임 창 클라이언트 영역: 화면 좌표 (x, y) + 크기 (w, h).
type Area = (i32, i32, i32, i32);

/// 카드 한 장을 읽은 결과.
#[derive(Clone, Copy, Default, PartialEq)]
enum Seen {
    /// 글자를 못 읽음 → "?"
    #[default]
    Unread,
    /// 읽었지만 데이터에 없는 증강 → S/A 가 아니므로 표시 안 함
    NotInData,
    Aug(u16),
}

#[derive(Clone, Copy, Default)]
struct Slot {
    signature: Option<Signature>,
    seen: Seen,
}

struct App {
    overlay: Overlay,
    layout: Layout,
    /// 개발용: 게임 대신 이 제목의 창에 붙는다 (--target "제목").
    target: Option<HSTRING>,
    /// Some = 증강 선택 화면이 열려 있음.
    cards: Option<[Slot; 3]>,
    /// 개발용 --cards 로 카드를 고정했으면 화면을 읽지 않는다.
    fixed_cards: bool,
    /// 지금 상태와 반대되는 판정이 연속으로 나온 횟수.
    flips: u8,
    next_watch: Instant,
    /// 처음 필요할 때 만들고 선택 화면이 닫히면 버린다.
    ocr: Option<Ocr>,
    /// 게임당 1회 조회해서 캐시. 게임 창이 사라지면 비운다. (--champion 키 로 고정 가능)
    champion: Option<data::Champ>,
    fixed_champion: bool,
    /// 이번 게임에서 고른 증강(picks.rs). 게임 창이 사라지면 비운다.
    picks: Picks,
    next_lookup: Instant,
    /// 백틱 단축키는 선택 화면 동안만 잡는다(평소 ` 입력을 막지 않게).
    hotkey: bool,
    hidden: bool,
    /// 보정을 취소하면 되돌릴 값.
    saved_layout: Layout,
    /// 보정 모드(Ctrl+Alt+F9): 선택 화면이 아니어도 가이드를 그리고 방향키로 카드 위치를 맞춘다.
    calibrating: bool,
    panel_left: bool,
    /// 지금 화면에 그려져 있는 상태. 이것이 바뀔 때만 다시 그린다.
    shown: Option<(Area, Option<usize>)>,
    interval: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn read_title(ocr: Option<&Ocr>, title: &Image) -> (Seen, Vec<String>) {
    let read = |img: &Image| ocr.and_then(|o| o.lines(img).ok()).unwrap_or_default();
    let mut lines = read(title);
    // 원래 크기에서 못 맞추면 2배로 다시 읽는다("전환: 골드" 의 "골드" 를 놓치는 경우). 2배로만 읽으면
    // 다른 제목("핏빛 코발트")을 틀려서 둘 다 쓴다.
    if data::match_title(&lines).is_none() {
        let again = read(&title.doubled());
        if data::match_title(&again).is_some() {
            lines = again;
        }
    }
    let seen = match data::match_title(&lines) {
        Some(aug) => Seen::Aug(aug),
        None if lines.is_empty() => Seen::Unread,
        None => Seen::NotInData,
    };
    (seen, lines)
}

impl App {
    fn game_window(&self) -> Option<HWND> {
        unsafe {
            match &self.target {
                Some(title) => FindWindowW(None, title),
                None => FindWindowW(GAME_CLASS, GAME_TITLE),
            }
            .ok()
        }
    }

    fn tick(&mut self) {
        let game = self.game_window();
        // 게임 창이 포그라운드가 아니면 화면을 읽지도, 그리지도 않는다.
        let area: Option<Area> = game.and_then(|g| unsafe {
            let (mut r, mut origin) = (RECT::default(), POINT::default());
            let visible = GetForegroundWindow() == g && !IsIconic(g).as_bool();
            (visible && GetClientRect(g, &mut r).is_ok() && ClientToScreen(g, &mut origin).as_bool() && r.bottom > 0)
                .then_some((origin.x, origin.y, r.right, r.bottom))
        });
        self.update_champion(game.is_some());
        if game.is_none() && !self.fixed_cards {
            self.picks.clear();
        }
        if !self.fixed_cards {
            match area {
                Some((x, y, w, h)) => {
                    if Instant::now() >= self.next_watch {
                        self.watch(&Source::Screen { x, y }, w as f32, h as f32);
                        let pace = if self.cards.is_some() { SELECTING_WATCH } else { IN_GAME_WATCH };
                        self.next_watch = Instant::now() + pace;
                    }
                }
                None => self.close(),
            }
        }

        let selecting = area.filter(|_| self.cards.is_some());
        if let (Some(area), Some(slots)) = (selecting, self.cards) {
            // 하위 비트 = 지난 확인 이후 눌린 적 있음(짧은 클릭도 잡는다)
            // 상위 비트 = 지금 눌려 있음. 33ms 마다 보므로 보통 클릭(50ms 이상)은 둘 중 하나로 잡힌다.
            if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8001 != 0 {
                if let Some(i) = self.hover(area) {
                    self.picks.click(match slots[i].seen {
                        Seen::Aug(aug) => Some(aug),
                        _ => None,
                    });
                }
            }
        }
        self.set_hotkey(selecting.is_some());
        match if self.calibrating { area } else { selecting } {
            Some(area) if !self.hidden || self.calibrating => {
                let state = (area, self.hover(area));
                if self.shown != Some(state) {
                    let (x, y, w, h) = area;
                    let scene = self.scene(w, h, state.1);
                    self.shown = self.overlay.show(x, y, w, h, &scene).ok().map(|_| state);
                }
            }
            _ => {
                self.overlay.hide();
                self.shown = None;
            }
        }

        let interval = match (game, self.shown) {
            // 숨김(`) 상태여도 선택 화면 동안은 클릭을 놓치지 않게 자주 본다
            _ if selecting.is_some() => HOVER_MS,
            (_, Some(_)) => HOVER_MS,
            (Some(_), None) => IN_GAME_MS,
            (None, None) => IDLE_MS,
        };
        if interval != self.interval {
            self.interval = interval;
            unsafe {
                SetTimer(Some(self.overlay.hwnd), TIMER, interval, None);
            }
        }
    }

    /// 선택 화면이 열렸는지/닫혔는지 보고, 열려 있으면 카드가 바뀌었는지(리롤) 본다.
    fn watch(&mut self, src: &Source, w: f32, h: f32) {
        let present = detect::selecting(&self.layout, w, h, &mut |r| src.grab(r));
        if present == self.cards.is_some() {
            self.flips = 0;
        } else {
            self.flips += 1;
            if self.flips < CONFIRMATIONS {
                return;
            }
            if !present {
                return self.close();
            }
            self.flips = 0;
            self.picks.opened();
            self.cards = Some(Default::default());
        }
        if present {
            self.read_cards(src, w, h);
        }
    }

    fn close(&mut self) {
        if let Some(slots) = self.cards {
            let on_screen: Vec<u16> = slots.iter().filter_map(|s| if let Seen::Aug(a) = s.seen { Some(a) } else { None }).collect();
            self.picks.closed(&on_screen, Instant::now());
        }
        self.cards = None;
        self.ocr = None;
        self.flips = 0;
    }

    /// 지문(아이콘+제목 dHash)이 바뀐 카드만 OCR 로 다시 읽는다.
    // ponytail: OCR 이 카드당 3~5ms 라 명세의 icon_cache.bin(디스크 캐시)은 만들지 않았다. OCR 이 느린 PC 가 나오면 추가.
    // ponytail: UI 스레드에서 동기 OCR. 카드가 바뀔 때만 돌므로 호버가 끊길 정도는 아님.
    fn read_cards(&mut self, src: &Source, w: f32, h: f32) -> [Vec<String>; 3] {
        let mut read: [Vec<String>; 3] = Default::default();
        let Some(mut slots) = self.cards else { return read };
        for (i, slot) in slots.iter_mut().enumerate() {
            let card = self.layout.card(i, w, h);
            let (Some(icon), Some(title)) = (src.grab(&detect::icon_area(&card)), src.grab(&detect::title_area(&card))) else {
                continue;
            };
            let signature = Signature(detect::dhash(&icon), detect::dhash(&title));
            if slot.signature.is_some_and(|old| old.same(&signature)) {
                continue;
            }
            if self.ocr.is_none() {
                self.ocr = Ocr::new().ok();
            }
            let (seen, lines) = read_title(self.ocr.as_ref(), &title);
            *slot = Slot { signature: Some(signature), seen };
            read[i] = lines;
            if let Seen::Aug(aug) = seen {
                self.picks.offered(aug, Instant::now());
            }
            self.shown = None;
        }
        self.cards = Some(slots);
        read
    }

    fn update_champion(&mut self, in_game: bool) {
        if self.fixed_champion {
            return;
        }
        if !in_game {
            self.champion = None;
        } else if self.champion.is_none() && Instant::now() >= self.next_lookup {
            self.champion = live_client::fetch_my_champion().and_then(|key| data::champion(&key));
            self.next_lookup = Instant::now() + CHAMPION_RETRY;
            self.shown = None;
        }
    }

    fn hover(&self, (x, y, w, h): Area) -> Option<usize> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p).ok()? };
        self.layout.hover((p.x - x) as f32, (p.y - y) as f32, w as f32, h as f32)
    }

    fn scene(&self, w: i32, h: i32, hover: Option<usize>) -> Scene {
        let (w, h) = (w as f32, h as f32);
        let slots = self.cards.unwrap_or_default();
        let cards: [(Rect, Card); 3] = std::array::from_fn(|i| {
            let card = match (slots[i].seen, &self.champion) {
                (Seen::Unread, _) => Card::Unknown,
                (Seen::Aug(aug), Some(champ)) => data::entry(champ, aug).map_or(Card::Ungraded, Card::Graded),
                _ => Card::Ungraded,
            };
            (self.layout.card(i, w, h), card)
        });
        let builds = data::with_general(self.champion.as_ref().map(data::builds).unwrap_or_default(), &self.picks.list);
        let augs = slots.map(|s| match s.seen {
            Seen::Aug(aug) => Some(aug),
            _ => None,
        });
        let best_i = data::best_card(&builds, &self.picks.list, &augs);
        let build_of = |aug| data::build_for(&builds, &self.picks.list, aug);
        let best = std::array::from_fn(|i| (best_i == Some(i)).then(|| augs[i].and_then(build_of).map_or(0, |b| b.name)));
        let has_info = |i: usize| matches!(cards[i].1, Card::Graded(_)) || augs[i].is_some_and(|a| build_of(a).is_some());
        let top_graded = (0..3).rev().filter_map(|i| match cards[i].1 {
            Card::Graded(e) => Some((e.grade as u8, i)),
            _ => None,
        });
        // 호버한 카드, 없으면 BEST 카드, 없으면 등급이 가장 높은 카드(같으면 가장 왼쪽).
        let shown = hover.filter(|&i| has_info(i)).or(best_i).or_else(|| top_graded.max().map(|(_, i)| i));
        // 패널: 카드 줄의 오른쪽(또는 왼쪽) 빈 곳. 폭도 높이에 비례.
        let (gap, width, edge) = (0.02 * h, 0.355 * h, 0.004 * w);
        let (l, r) = if self.panel_left {
            let r = cards[0].0.l - gap;
            ((r - width).max(edge), r)
        } else {
            let l = cards[2].0.r + gap;
            (l, (l + width).min(w - edge))
        };
        let panel = shown.filter(|_| !self.calibrating).and_then(|i| {
            let aug = augs[i]?;
            let build = build_of(aug).cloned();
            let partners = self
                .picks
                .list
                .iter()
                .copied()
                .filter(|&p| data::sets(p) & data::sets(aug) != 0 || build.as_ref().is_some_and(|b| b.augs.contains(&p) && b.augs.contains(&aug)))
                .collect();
            let next = build.iter().flat_map(|b| &b.augs).copied().filter(|&a| a != aug && !self.picks.list.contains(&a)).take(4).collect();
            let entry = match cards[i].1 {
                Card::Graded(e) => Some(e),
                _ => None,
            };
            let panel = Panel { aug, entry, build, partners, next, picked: self.picks.list.clone() };
            Some((Rect { l, t: cards[0].0.t, r, b: h }, panel))
        });
        Scene { guide: self.calibrating, cards, best, panel }
    }

    fn set_hotkey(&mut self, on: bool) {
        if on == self.hotkey {
            return;
        }
        self.hotkey = on;
        unsafe {
            if on {
                let _ = RegisterHotKey(Some(self.overlay.hwnd), HK_TOGGLE, MOD_NOREPEAT, VK_OEM_3.0 as u32);
            } else {
                let _ = UnregisterHotKey(Some(self.overlay.hwnd), HK_TOGGLE);
            }
        }
    }
}

impl App {
    fn set_calibrating(&mut self, on: bool, save: bool) {
        if on == self.calibrating {
            return;
        }
        self.calibrating = on;
        self.shown = None;
        let hwnd = Some(self.overlay.hwnd);
        unsafe {
            for (m, modifier) in [HOT_KEY_MODIFIERS(0), MOD_SHIFT, MOD_CONTROL].into_iter().enumerate() {
                for (d, key) in [VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN].into_iter().enumerate() {
                    let id = HK_ARROWS + (m * 4 + d) as i32;
                    let _ = if on { RegisterHotKey(hwnd, id, modifier, key.0 as u32) } else { UnregisterHotKey(hwnd, id) };
                }
            }
            let _ = if on { RegisterHotKey(hwnd, HK_SAVE, MOD_NOREPEAT, VK_RETURN.0 as u32) } else { UnregisterHotKey(hwnd, HK_SAVE) };
        }
        if on {
            self.saved_layout = self.layout;
        } else if save {
            config::save_layout(&self.layout);
        } else {
            self.layout = self.saved_layout;
        }
    }

    fn nudge(&mut self, id: i32) {
        let (modifier, direction) = ((id - HK_ARROWS) / 4, (id - HK_ARROWS) % 4);
        let step = if direction % 2 == 0 { -CALIBRATE_STEP } else { CALIBRATE_STEP };
        let horizontal = direction < 2;
        let l = &mut self.layout;
        match (modifier, horizontal) {
            (0, true) => l.center_x += step,
            (0, false) => l.card_top += step,
            (1, true) => l.card_w = (l.card_w + step).max(0.05),
            (1, false) => l.card_h = (l.card_h + step).max(0.05),
            (_, true) => l.spacing = (l.spacing + step).max(0.05),
            _ => {}
        }
        self.shown = None;
    }
}

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with_borrow_mut(|app| app.as_mut().map(f));
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match (msg, wparam.0 as i32) {
        (WM_TIMER, _) => with_app(App::tick),
        (WM_HOTKEY, HK_TOGGLE) => with_app(|app| {
            app.hidden = !app.hidden;
            app.tick();
        }),
        (WM_HOTKEY, HK_CALIBRATE) => with_app(|app| {
            app.set_calibrating(!app.calibrating, false);
            app.tick();
        }),
        (WM_HOTKEY, HK_SAVE) => with_app(|app| {
            app.set_calibrating(false, true);
            app.tick();
        }),
        (WM_HOTKEY, id) if (HK_ARROWS..HK_ARROWS + 12).contains(&id) => with_app(|app| {
            app.nudge(id);
            app.tick();
        }),
        (WM_HOTKEY, HK_UNDO_PICK) => with_app(|app| {
            app.picks.undo();
            app.shown = None;
            app.tick();
        }),
        (WM_HOTKEY, HK_QUIT) => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        (WM_DESTROY, _) => unsafe { PostQuitMessage(0) },
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
    LRESULT(0)
}

/// 개발용 옵션:
///   --target "창 제목"            게임 대신 이 창에 붙는다
///   --champion Brand              Live Client 대신 챔피언 고정
///   --cards "이름,이름,이름"      카드 3장 고정(화면 인식 대신). 모르는 이름은 "?"
///   --image frame.png             화면 대신 이미지(클라이언트 영역 전체)로 감지·인식하고 결과를 출력
///   --render out.bmp [폭 높이]    화면에 띄우지 않고 그림만 저장 (--hover 0~2 로 호버 지정)
///   --guide                       --render 에 보정 모드 가이드를 함께 그린다
///   --picked "이름,이름"          이미 고른 증강 고정 (BEST 확인용)
///   --whoami                      Live Client 에서 읽은 챔피언 키 출력
///   --dump                        Live Client 전체 데이터를 exe 옆 allgamedata.json 으로 저장(게임 중에)
///   --check-update                업데이트만 확인하고 어떤 데이터를 쓰는지 출력
pub fn run() -> Result<()> {
    assert!(data::valid(data::embedded()), "data.bin 형식 오류");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str, n: usize| args.iter().position(|a| a == name).and_then(|i| args.get(i + n));
    let config = config::load();
    // 개발용 오프라인 확인(--image, --render)은 늘 exe 에 든 데이터로 한다.
    if ["--image", "--render", "--whoami", "--dump"].iter().all(|o| arg(o, 0).is_none()) {
        println!("{}", update::load(&config.update_url));
    }
    if arg("--check-update", 0).is_some() {
        return Ok(());
    }
    if arg("--dump", 0).is_some() {
        let path = std::env::current_exe()?.with_file_name("allgamedata.json");
        let body = live_client::all_game_data().unwrap_or_default();
        std::fs::write(&path, &body)?;
        println!("{} ({} bytes)", path.display(), body.len());
        return Ok(());
    }
    if arg("--whoami", 0).is_some() {
        println!("{:?}", live_client::fetch_my_champion());
        return Ok(());
    }
    let champion = arg("--champion", 1).and_then(|key| data::champion(key));
    let cards = arg("--cards", 1).map(|list| {
        let mut names = list.split(',').map(data::aug_by_name);
        std::array::from_fn(|_| Slot { signature: None, seen: names.next().flatten().map_or(Seen::Unread, Seen::Aug) })
    });
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)?;
        let mut overlay = Overlay::new(Some(wndproc))?;
        overlay.alpha = (config.opacity as u32 * 255 / 100) as u8;
        overlay.scale = config.scale;
        let mut app = App {
            overlay,
            layout: config.layout,
            saved_layout: config.layout,
            calibrating: arg("--guide", 0).is_some(),
            panel_left: config.panel_left,
            target: arg("--target", 1).map(HSTRING::from),
            cards,
            fixed_cards: cards.is_some(),
            flips: 0,
            next_watch: Instant::now(),
            ocr: None,
            champion,
            fixed_champion: champion.is_some(),
            picks: Picks::with(arg("--picked", 1).map(|l| l.split(',').filter_map(data::aug_by_name).collect()).unwrap_or_default()),
            next_lookup: Instant::now(),
            hotkey: false,
            hidden: false,
            shown: None,
            interval: 0,
        };

        let size = |n, default| arg("--render", n).and_then(|v| v.parse().ok()).unwrap_or(default);
        let (mut w, mut h) = (size(2, 1920), size(3, 1080));
        if let Some(path) = arg("--image", 1) {
            let img = capture::load(path)?;
            (w, h) = (img.w as i32, img.h as i32);
            let src = Source::File(img);
            let started = Instant::now();
            // 실제와 같은 경로: 연속 2회 확인 후 카드 읽기
            let mut lines = Default::default();
            for _ in 0..CONFIRMATIONS {
                app.watch(&src, w as f32, h as f32);
            }
            if app.cards.is_some() {
                app.cards = Some(Default::default());
                lines = app.read_cards(&src, w as f32, h as f32);
            }
            let names: Vec<String> = app
                .cards
                .iter()
                .flatten()
                .map(|slot| match slot.seen {
                    Seen::Unread => "?".into(),
                    Seen::NotInData => "-".into(),
                    Seen::Aug(aug) => String::from_utf16_lossy(&data::text(data::aug_name(aug))),
                })
                .collect();
            println!("selecting={} cards={}", app.cards.is_some(), names.join(","));
            println!("  ocr={lines:?} took={:?}", started.elapsed());
        }
        if let Some(path) = arg("--render", 1) {
            let scene = app.scene(w, h, arg("--hover", 1).and_then(|v| v.parse().ok()));
            return app.overlay.save_bmp(path, w, h, &scene);
        }
        if arg("--image", 0).is_some() {
            return Ok(());
        }

        for (id, key) in [(HK_QUIT, VK_F10), (HK_CALIBRATE, VK_F9), (HK_UNDO_PICK, VK_F8)] {
            RegisterHotKey(
                Some(app.overlay.hwnd),
                id,
                HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0),
                key.0 as u32,
            )?;
        }
        APP.set(Some(app));
        with_app(App::tick);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
