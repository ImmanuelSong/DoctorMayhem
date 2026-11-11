//! data.bin (MYH1) 읽기 전용 조회. 레이아웃은 data/build_data.py 참고.
//! exe 에 든 사본을 쓰다가, 시작할 때 받은 새 데이터가 검사를 통과하면 그것으로 바꾼다(use_blob).

use std::sync::OnceLock;

static EMBEDDED: &[u8] = include_bytes!("../data/data.bin");
static ACTIVE: OnceLock<&'static [u8]> = OnceLock::new();

fn blob() -> &'static [u8] {
    ACTIVE.get().copied().unwrap_or(EMBEDDED)
}

pub fn embedded() -> &'static [u8] {
    EMBEDDED
}

/// 검사를 통과한 데이터로 바꾼다(프로그램 시작 때 한 번). 통과 못 하면 false, 지금 데이터 유지.
pub fn use_blob(b: Vec<u8>) -> bool {
    valid(&b) && ACTIVE.set(Box::leak(b.into_boxed_slice())).is_ok()
}

/// 받은 파일이 깨졌거나 형식이 달라도 조회 중에 범위를 벗어나지 않는지 전부 확인한다.
pub fn valid(b: &[u8]) -> bool {
    let u16a = |o: usize| b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as usize);
    let u32a = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize);
    let check = || -> Option<bool> {
        let h: Vec<usize> = (0..19).map(|i| u32a(4 + i * 4)).collect::<Option<_>>()?;
        let [ver, n_aug, n_champ, n_entry, n_item, n_name, off_pool, units, off_aug, off_champ, off_entry, off_item, off_name, patch, credit, n_build, off_build, off_list, _rev] =
            h[..]
        else {
            return None;
        };
        let fits = |off: usize, n: usize, size: usize| off.checked_add(n.checked_mul(size)?).filter(|&e| e <= b.len());
        let pool_end = fits(off_pool, units, 2)?;
        let s = |off: usize| off < units;
        let aug = |a: usize| a < n_aug;
        let item_or_none = |i: usize| i == 0xFFFF || i < n_item;
        let aug_or_none = |a: usize| a == 0xFFFF || aug(a);
        let list_len = b.len().checked_sub(off_list)? / 2;
        let ok = &b[..4] == b"MYH1"
            && ver == 2
            && units > 0
            && u16a(pool_end - 2)? == 0
            && fits(off_aug, n_aug, AUG_SIZE).is_some()
            && fits(off_champ, n_champ, CHAMP_SIZE).is_some()
            && fits(off_entry, n_entry, ENTRY_SIZE).is_some()
            && fits(off_item, n_item, 4).is_some()
            && fits(off_name, n_name, 8).is_some()
            && fits(off_build, n_build, BUILD_SIZE).is_some()
            && s(patch)
            && s(credit)
            && (0..n_aug).all(|i| [4, 8].iter().all(|&f| u32a(off_aug + i * AUG_SIZE + f).is_some_and(s)))
            && (0..n_item).all(|i| u32a(off_item + i * 4).is_some_and(s))
            && (0..n_champ).all(|i| {
                let o = off_champ + i * CHAMP_SIZE;
                u32a(o + 4).is_some_and(s)
                    && u32a(o + 8).zip(u16a(o + 12)).is_some_and(|(begin, n)| begin + n <= n_entry)
                    && (i == 0 || u32a(o - CHAMP_SIZE) < u32a(o))
            })
            && (0..n_entry).all(|i| {
                let o = off_entry + i * ENTRY_SIZE;
                u16a(o).is_some_and(aug)
                    && [4, 8, 12].iter().all(|&f| u32a(o + f).is_some_and(s))
                    && (0..3).all(|k| u16a(o + 16 + k * 2).is_some_and(aug_or_none) && u16a(o + 22 + k * 2).is_some_and(item_or_none))
            })
            && (0..n_name).all(|i| u32a(off_name + i * 8).is_some_and(s) && u32a(off_name + i * 8 + 4).is_some_and(|a| a == NO_DATA as usize || aug(a)))
            && (0..n_build).all(|i| {
                let o = off_build + i * BUILD_SIZE;
                let (na, ni) = (b[o + 14] as usize, b[o + 15] as usize);
                u32a(o + 4).is_some_and(s)
                    && u32a(o + 8).is_some_and(s)
                    && u16a(o + 12).is_some_and(|lo| {
                        lo + na + ni <= list_len
                            && (0..na).all(|k| u16a(off_list + (lo + k) * 2).is_some_and(aug))
                            && (0..ni).all(|k| u16a(off_list + (lo + na + k) * 2).is_some_and(|x| x < n_item))
                    })
            });
        Some(ok)
    };
    b.len() >= 80 && check() == Some(true)
}

/// 데이터 수정 시각(유닉스 초). 클수록 새 데이터.
pub fn revision(b: &[u8]) -> u32 {
    b.get(76..80).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

pub const NONE: u16 = 0xFFFF;
/// 이름 색인에서: 데이터에 없는 증강.
const NO_DATA: u32 = 0xFFFF_FFFF;
pub const ENT_VERIFY: u8 = 1;
pub const ENT_COMBO: u8 = 2;
const AUG_SIZE: usize = 16;
const CHAMP_SIZE: usize = 16;
const ENTRY_SIZE: usize = 28;
const BUILD_SIZE: usize = 16;

fn u16_at(o: usize) -> u16 {
    let b = blob();
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(o: usize) -> u32 {
    let b = blob();
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn hdr(i: usize) -> usize {
    u32_at(4 + i * 4) as usize
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    A = 1,
    S = 2,
}

#[derive(Clone, Copy)]
pub struct Champ {
    hash: u32,
    begin: usize,
    count: usize,
}

#[derive(Clone, Copy)]
pub struct Entry {
    pub grade: Grade,
    pub flags: u8,
    pub short: u32,
    pub why: u32,
    pub tip: u32,
    pub pairs: [u16; 3],
    pub items: [u16; 3],
}


/// 풀 오프셋 → UTF-16 문자열 (끝의 \0 제외).
pub fn text(off: u32) -> Vec<u16> {
    let base = hdr(6) + off as usize * 2;
    (0..).map(|i| u16_at(base + i * 2)).take_while(|&c| c != 0).collect()
}

/// 영숫자만 소문자로 → FNV-1a. build_data.py 의 norm_key + fnv1a 와 같아야 함.
fn key_hash(key: &str) -> u32 {
    key.bytes()
        .filter(u8::is_ascii_alphanumeric)
        .fold(0x811C_9DC5u32, |h, b| (h ^ b.to_ascii_lowercase() as u32).wrapping_mul(0x0100_0193))
}

pub fn champion(key: &str) -> Option<Champ> {
    let (base, h) = (hdr(9), key_hash(key));
    let (mut lo, mut hi) = (0, hdr(2));
    while lo < hi {
        let mid = (lo + hi) / 2;
        let o = base + mid * CHAMP_SIZE;
        match u32_at(o).cmp(&h) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Equal => {
                return Some(Champ { hash: h, begin: u32_at(o + 8) as usize, count: u16_at(o + 12) as usize })
            }
        }
    }
    None
}

pub fn entry(champ: &Champ, aug: u16) -> Option<Entry> {
    (champ.begin..champ.begin + champ.count).map(|i| hdr(10) + i * ENTRY_SIZE).find(|&o| u16_at(o) == aug).map(|o| Entry {
        grade: if blob()[o + 2] == 2 { Grade::S } else { Grade::A },
        flags: blob()[o + 3],
        short: u32_at(o + 4),
        why: u32_at(o + 8),
        tip: u32_at(o + 12),
        pairs: [u16_at(o + 16), u16_at(o + 18), u16_at(o + 20)],
        items: [u16_at(o + 22), u16_at(o + 24), u16_at(o + 26)],
    })
}

/// 빌드 방향: 서로 시너지가 나는 증강 묶음(우선순위 순) + 그에 맞는 아이템(사는 순서). data/builds.json.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Build {
    pub name: u32,
    pub why: u32,
    pub augs: Vec<u16>,
    pub items: Vec<u16>,
}

/// 챔피언의 빌드들, 주력부터.
pub fn builds(champ: &Champ) -> Vec<Build> {
    (0..hdr(15))
        .map(|i| hdr(16) + i * BUILD_SIZE)
        .filter(|&o| u32_at(o) == champ.hash)
        .map(|o| {
            let (list, na, ni) = (hdr(17) + u16_at(o + 12) as usize * 2, blob()[o + 14] as usize, blob()[o + 15] as usize);
            let ids: Vec<u16> = (0..na + ni).map(|i| u16_at(list + i * 2)).collect();
            Build { name: u32_at(o + 4), why: u32_at(o + 8), augs: ids[..na].to_vec(), items: ids[na..].to_vec() }
        })
        .collect()
}

/// 증강이 속한 세트(같은 세트를 모으면 보너스) 비트마스크.
pub fn sets(aug: u16) -> u16 {
    u16_at(hdr(8) + aug as usize * AUG_SIZE + 14)
}

/// 이미 고른 증강과 지금 카드 3장을 보고 고를 카드(BEST)를 정한다.
/// 카드 점수 = 그 카드가 든 빌드에서의 순위 + 그 빌드에 이미 고른 증강 수 × 12(고른 방향을 이어 감)
///           + 주력 빌드 3 + 같은 세트의 고른 증강마다 6. 어느 빌드에도 없고 세트도 안 겹치면 후보가 아니다.
// ponytail: 가중치는 손으로 정한 값. 실제 판에서 이상하면 여기만 고치면 된다.
pub fn best_card(builds: &[Build], picked: &[u16], cards: &[Option<u16>; 3]) -> Option<usize> {
    let score = |a: u16| -> Option<i32> {
        if picked.contains(&a) {
            return None;
        }
        let from_builds = builds.iter().enumerate().filter_map(|(i, b)| {
            let rank = b.augs.iter().position(|&x| x == a)? as i32;
            Some(20 - rank + 12 * picks_in(b, picked) + if i == 0 { 3 } else { 0 })
        });
        let set = 6 * picked.iter().filter(|&&p| sets(p) & sets(a) != 0).count() as i32;
        from_builds.max().map(|s| s + set).or((set > 0).then_some(set))
    };
    (0..3).filter_map(|i| Some((score(cards[i]?)?, std::cmp::Reverse(i)))).max().map(|(_, i)| i.0)
}

fn picks_in(b: &Build, picked: &[u16]) -> i32 {
    picked.iter().filter(|p| b.augs.contains(p)).count() as i32
}

/// 카드(증강 aug)를 고르면 따라갈 빌드: aug 가 든 빌드 중 고른 증강이 가장 많이 든 것(같으면 주력 쪽).
/// aug 가 어느 빌드에도 없으면 고른 증강이 가장 많이 든 빌드.
pub fn build_for<'a>(builds: &'a [Build], picked: &[u16], aug: u16) -> Option<&'a Build> {
    let best = |only_with_aug: bool| {
        let rank = |(i, b): &(usize, &Build)| (picks_in(b, picked), std::cmp::Reverse(*i));
        builds.iter().enumerate().filter(|(_, b)| !only_with_aug || b.augs.contains(&aug)).max_by_key(rank).map(|(_, b)| b)
    };
    best(true).or_else(|| best(false))
}

/// 이름 정규화: 한글 음절 + ASCII 영숫자(소문자)만. build_data.py 의 norm_name 과 같아야 함.
pub fn normalize(s: &str) -> Vec<u16> {
    s.chars()
        .filter(|c| ('가'..='힣').contains(c) || c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase() as u16)
        .collect()
}

/// 한국어 증강 이름(공백·특수문자 차이는 무시) → 증강 번호. 삭제된 증강은 None.
pub fn aug_by_name(name: &str) -> Option<u16> {
    let want = normalize(name);
    let aug = (0..hdr(5)).map(|i| hdr(12) + i * 8).find(|&o| text(u32_at(o)) == want).map(|o| u32_at(o + 4))?;
    (aug != NO_DATA).then_some(aug as u16)
}
fn edit_distance(a: &[u16], b: &[u16]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = prev + (ca != cb) as usize;
            prev = row[j + 1];
            row[j + 1] = cost.min(prev + 1).min(row[j] + 1);
        }
    }
    row[b.len()]
}

/// OCR 로 읽은 줄들에서 증강을 찾는다. 줄 하나씩, 그리고 전부 이어 붙인 것(제목이 두 줄일 때)을 맞춰 본다.
/// 이름 색인에는 게임의 모든 증강 이름이 들어 있어서, 등급 데이터가 없는 증강(예: "빵과 잼")도
/// 자기 이름으로 맞는다 — 비슷한 이름("빵과 버터")으로 잘못 잡히지 않는다.
/// 틀려도 되는 글자 수는 이름 길이의 1/4 까지만(3글자 이하는 완전히 같아야 함).
pub fn match_title<S: AsRef<str>>(lines: &[S]) -> Option<u16> {
    let joined: String = lines.iter().map(AsRef::as_ref).collect();
    let candidates: Vec<Vec<u16>> =
        lines.iter().map(AsRef::as_ref).chain([joined.as_str()]).map(normalize).filter(|c| !c.is_empty()).collect();
    (0..hdr(5))
        .map(|i| hdr(12) + i * 8)
        .filter_map(|o| {
            let name = text(u32_at(o));
            let d = candidates.iter().map(|c| edit_distance(c, &name)).min()?;
            (d <= name.len() / 4).then_some((d, u32_at(o + 4)))
        })
        .min()
        .and_then(|(_, aug)| (aug != NO_DATA).then_some(aug as u16))
}

pub fn aug_name(aug: u16) -> u32 {
    u32_at(hdr(8) + aug as usize * AUG_SIZE + 4)
}
pub fn item_name(item: u16) -> u32 {
    u32_at(hdr(11) + item as usize * 4)
}
pub fn credit() -> u32 {
    hdr(14) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(off: u32) -> String {
        String::from_utf16(&text(off)).unwrap()
    }

    #[test]
    fn brand_conduit_is_s() {
        assert!(valid(embedded()));
        // 잘린 파일, 엉뚱한 파일, 오프셋 하나가 범위를 벗어난 파일은 거부
        assert!(!valid(&embedded()[..embedded().len() / 2]));
        assert!(!valid(b"MYH1 not really data, but long enough to read a header from it......"));
        let mut bad = embedded().to_vec();
        let off_aug = u32::from_le_bytes(bad[36..40].try_into().unwrap()) as usize;
        bad[off_aug + 4..off_aug + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!valid(&bad));
        let brand = champion("Brand").unwrap();
        let e = entry(&brand, aug_by_name("지옥의전도체").unwrap()).unwrap();
        assert_eq!(e.grade, Grade::S);
        assert_eq!(s(e.short), "화상으로 쿨 계속 감소");
        assert_eq!(s(aug_name(e.pairs[0])), "마도사의 주스 상자");
        assert_eq!(s(item_name(e.items[0])), "리안드리의 고통");
        assert_eq!(e.pairs[2], NONE);
        assert_eq!(s(credit()), "Data: arammayhem.com (CC BY 4.0)");
    }

    #[test]
    fn best_card_follows_picks() {
        let aug = |n: &str| aug_by_name(n).unwrap();
        let b = |augs: &[&str]| Build { augs: augs.iter().map(|&n| aug(n)).collect(), ..Default::default() };
        let builds = [b(&["지옥의 전도체", "유레카", "마법 미사일"]), b(&["보석 건틀릿", "취약", "축소 엔진"])];
        let cards = [Some(aug("마법 미사일")), Some(aug("취약")), Some(aug("빵과 잼"))];
        assert_eq!(best_card(&builds, &[], &cards), Some(0)); // 고른 게 없으면 주력 빌드
        assert_eq!(best_card(&builds, &[aug("보석 건틀릿")], &cards), Some(1)); // 고른 증강의 빌드를 이어 감
        assert_eq!(best_card(&builds, &[], &[Some(aug("빵과 잼")), None, None]), None); // 어느 빌드에도 없음
        assert_eq!(build_for(&builds, &[aug("보석 건틀릿")], aug("빵과 잼")), Some(&builds[1]));
        assert_eq!(build_for(&builds, &[], aug("마법 미사일")), Some(&builds[0]));
        // 실제 데이터: 모든 챔피언에 빌드가 있고, 빌드의 증강·아이템 번호가 범위 안
        let brand = builds_of("Brand");
        assert!(!brand.is_empty() && brand[0].augs.contains(&aug("지옥의 전도체")));
        assert_eq!(s(item_name(brand[0].items[0])), "리안드리의 고통");
        // 애쉬: 고른 게 없으면 주력(온힛) 빌드의 양손잡이, 주문 분산을 골랐으면 W 빌드의 대마법사
        let ashe = builds_of("Ashe");
        let cards = [Some(aug("양손잡이")), Some(aug("대마법사")), Some(aug("빵과 잼"))];
        assert_eq!(best_card(&ashe, &[], &cards), Some(0));
        assert_eq!(best_card(&ashe, &[aug("주문 분산")], &cards), Some(1));
        assert_eq!(build_for(&ashe, &[aug("주문 분산")], aug("축소 엔진")), Some(&ashe[1])); // 두 빌드 모두에 있음
        // 세트: 폭죽 세트(마법 미사일·쌍둥이 불꽃)
        assert_ne!(sets(aug("마법 미사일")) & sets(aug("쌍둥이 불꽃")), 0);
    }

    fn builds_of(key: &str) -> Vec<Build> {
        builds(&champion(key).unwrap())
    }

    #[test]
    fn key_forms() {
        // Live Client 가 주는 형태(공백·특수문자 포함)도 같은 챔피언으로 조회돼야 함
        for k in ["MonkeyKing", "monkeyking", "Kai'Sa", "Miss Fortune", "KSante", "Belveth"] {
            assert!(champion(k).is_some(), "{k}");
        }
        assert!(champion("NotAChampion").is_none());
        assert_eq!(aug_by_name("퀘스트: 우글렛의 마녀 모자"), aug_by_name("퀘스트 우글렛의마녀모자"));
        assert!(aug_by_name("3연발").is_some());
        assert!(aug_by_name("선동").is_none()); // 삭제된 증강
    }

    #[test]
    fn ocr_lines_match_strictly() {
        let name = |lines: &[&str]| match_title(lines).map(|a| s(aug_name(a)));
        assert_eq!(name(&["마도사의 주스 상자", "지원"]).as_deref(), Some("마도사의 주스 상자"));
        assert_eq!(name(&["마도사의 주스 삼자"]).as_deref(), Some("마도사의 주스 상자")); // 한 글자 오독
        assert_eq!(name(&["퀘스트: 우글렛의", "마녀 모자"]).as_deref(), Some("퀘스트: 우글렛의 마녀 모자")); // 두 줄 제목
        assert_eq!(name(&["부식", "보조 피해량"]).as_deref(), Some("부식"));
        // 게임 화면의 공식 이름이 데이터 이름과 다른 증강
        assert_eq!(name(&["보강"]).as_deref(), Some("2연속 방어"));
        assert_eq!(name(&["우글렛의 마녀 모자"]).as_deref(), Some("퀘스트: 우글렛의 마녀 모자"));
        assert_eq!(name(&["강철 같은 심장"]).as_deref(), Some("퀘스트: 강철 같은 심장"));
        // 등급 데이터가 없는 증강도 자기 이름으로 잡히고, 비슷한 이름으로 잡히면 안 된다
        assert_eq!(name(&["빵과 잼", "쿨감"]).as_deref(), Some("빵과 잼"));
        assert_eq!(name(&["빵과 젬"]), None); // 3글자 이름은 오독을 고치지 않는다 — 빵과 버터가 되면 안 됨
        assert_eq!(name(&["빵과 버터"]).as_deref(), Some("빵과 버터"));
        assert_eq!(name(&["최상급 조준경 부착"]).as_deref(), Some("최상급 조준경 부착"));
        assert_eq!(name(&["전환: 골드", "보조"]).as_deref(), Some("전환: 골드"));
        assert_eq!(name(&["점멸 난사"]).as_deref(), Some("점멸 난사"));
        assert_eq!(name(&[]), None);
    }
}
