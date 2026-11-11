//! 패치 후 새 데이터 받기. 서버 없이 정적 파일 하나(HTTPS)만 있으면 된다.
//! 시작할 때 한 번: 받은 것 → exe 옆에 저장해 둔 것 → exe 에 든 것 순서로, 검사를 통과하고 가장 새것을 쓴다.

use crate::data;
use crate::live_client::https_get;
use std::path::PathBuf;
use windows::core::{HSTRING, PCWSTR};

/// 기본 업데이트 주소: GitHub 저장소의 data/data.bin(main 브랜치에 올리면 바로 반영, 캐시 몇 분).
/// settings.ini [update] url= 로 바꿀 수 있고, url=- 를 쓰면 업데이트를 끈다.
pub const DEFAULT_URL: &str = "https://raw.githubusercontent.com/ImmanuelSong/DoctorMayhem/main/data/data.bin";
const TIMEOUT_MS: i32 = 3000;
const MAX_BYTES: usize = 8 << 20;

fn cache_path() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.with_file_name("data.bin"))
}

/// "https://host/path" 만 받는다.
fn download(url: &str) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_at(rest.find('/')?);
    let (host, path) = (HSTRING::from(host), HSTRING::from(path));
    https_get(PCWSTR(host.as_ptr()), 443, PCWSTR(path.as_ptr()), false, TIMEOUT_MS, MAX_BYTES)
}

/// 어떤 데이터를 쓰게 됐는지 한 줄로 돌려준다(콘솔 출력용).
pub fn load(url: &str) -> String {
    let fetched = (!url.is_empty()).then(|| download(url)).flatten().filter(|b| data::valid(b));
    let cached = cache_path().and_then(|p| std::fs::read(p).ok()).filter(|b| data::valid(b));
    if let (Some(new), Some(path)) = (&fetched, cache_path()) {
        if cached.as_ref() != Some(new) {
            let tmp = path.with_extension("tmp");
            let _ = std::fs::write(&tmp, new).and_then(|_| std::fs::rename(&tmp, &path));
        }
    }
    let downloaded = fetched.is_some();
    let best = [fetched, cached].into_iter().flatten().max_by_key(|b| data::revision(b));
    match best.filter(|b| data::revision(b) > data::revision(data::embedded())) {
        Some(b) => {
            let rev = data::revision(&b);
            data::use_blob(b);
            format!("데이터: 업데이트 사용 (rev {rev}{})", if downloaded { "" } else { ", 오프라인 — 저장해 둔 것" })
        }
        None => format!(
            "데이터: 프로그램에 든 것 (rev {}){}",
            data::revision(data::embedded()),
            if url.is_empty() { ", 업데이트 주소 없음" } else if downloaded { "" } else { ", 업데이트 확인 실패" }
        ),
    }
}
