//! Riot 공식 로컬 API(Live Client Data, 127.0.0.1:2999)에서 내 챔피언 키를 읽는다. 읽기 전용.

/// 최상위 배열 안의 객체들을 잘라낸다(문자열 안의 중괄호는 무시).
fn top_level_objects(json: &str) -> impl Iterator<Item = &str> {
    let (mut depth, mut in_str, mut esc, mut start) = (0, false, false, 0);
    json.bytes().enumerate().filter_map(move |(i, b)| {
        if in_str {
            (esc, in_str) = (!esc && b == b'\\', esc || b != b'"');
        } else if b == b'"' {
            in_str = true;
        } else if b == b'{' {
            depth += 1;
            if depth == 1 {
                start = i;
            }
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(&json[start..=i]);
            }
        }
        None
    })
}

fn string_value<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let rest = &obj[obj.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

/// playerlist 응답에서 `quoted_riot_id`(activeplayername 응답 원문, 따옴표 포함)의 챔피언 키.
/// rawChampionName 은 "game_character_displayname_Brand" 형태라 마지막 토막만 쓴다.
pub fn my_champion<'a>(player_list: &'a str, quoted_riot_id: &str) -> Option<&'a str> {
    let id = quoted_riot_id.trim();
    if id.len() < 3 || !id.starts_with('"') {
        return None;
    }
    let me = top_level_objects(player_list).find(|o| o.contains(id))?;
    string_value(me, "rawChampionName")?.rsplit('_').next()
}

#[cfg(windows)]
pub fn fetch_my_champion() -> Option<String> {
    let name = get(windows::core::w!("/liveclientdata/activeplayername"))?;
    let list = get(windows::core::w!("/liveclientdata/playerlist"))?;
    my_champion(&list, &name).map(str::to_owned)
}

/// 개발용: 게임이 주는 전체 데이터(JSON). 증강 정보가 들어 있는지 확인하는 용도.
#[cfg(windows)]
pub fn all_game_data() -> Option<String> {
    let body = https_get(windows::core::w!("127.0.0.1"), 2999, windows::core::w!("/liveclientdata/allgamedata"), true, 2000, 8 << 20)?;
    String::from_utf8(body).ok()
}

// ponytail: UI 스레드에서 동기 호출(타임아웃 500ms, 게임당 1회 성공하면 끝). 끊김이 보이면 스레드로.
#[cfg(windows)]
fn get(path: windows::core::PCWSTR) -> Option<String> {
    let body = https_get(windows::core::w!("127.0.0.1"), 2999, path, true, 500, 1 << 20)?;
    String::from_utf8(body).ok().filter(|s| !s.is_empty())
}

/// HTTPS GET. local = 게임의 로컬 API(자체 서명 인증서라 검증을 끈다). 그 밖에는 인증서를 정상 검증한다.
/// 응답 코드가 200 이 아니거나 max 바이트를 넘으면 None.
#[cfg(windows)]
pub fn https_get(host: windows::core::PCWSTR, port: u16, path: windows::core::PCWSTR, local: bool, timeout_ms: i32, max: usize) -> Option<Vec<u8>> {
    use windows::core::w;
    use windows::Win32::Networking::WinHttp::*;
    const IGNORE_CERT: u32 = SECURITY_FLAG_IGNORE_UNKNOWN_CA
        | SECURITY_FLAG_IGNORE_CERT_CN_INVALID
        | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
        | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
    unsafe {
        let access = if local { WINHTTP_ACCESS_TYPE_NO_PROXY } else { WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY };
        let session = WinHttpOpen(w!("mayhem-overlay"), access, None, None, 0);
        let _ = WinHttpSetTimeouts(session, timeout_ms, timeout_ms, timeout_ms, timeout_ms);
        let connect = WinHttpConnect(session, host, port, 0);
        let request = WinHttpOpenRequest(connect, w!("GET"), path, None, None, std::ptr::null(), WINHTTP_FLAG_SECURE);
        let (mut status, mut len) = (0u32, 4u32);
        let ok = (!local || WinHttpSetOption(Some(request), WINHTTP_OPTION_SECURITY_FLAGS, Some(&IGNORE_CERT.to_ne_bytes())).is_ok())
            && WinHttpSendRequest(request, None, None, 0, 0, 0).is_ok()
            && WinHttpReceiveResponse(request, std::ptr::null_mut()).is_ok()
            && WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                None,
                Some((&mut status as *mut u32).cast()),
                &mut len,
                std::ptr::null_mut(),
            )
            .is_ok()
            && status == 200;
        let (mut body, mut buf, mut n) = (Vec::new(), [0u8; 8192], 0u32);
        while ok && body.len() <= max && WinHttpReadData(request, buf.as_mut_ptr().cast(), buf.len() as u32, &mut n).is_ok() && n > 0 {
            body.extend_from_slice(&buf[..n as usize]);
        }
        for handle in [request, connect, session] {
            let _ = WinHttpCloseHandle(handle);
        }
        (ok && body.len() <= max).then_some(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"[
      {"championName":"애쉬","items":[{"displayName":"} \"{"}],"rawChampionName":"game_character_displayname_Ashe","riotId":"Other#KR1","scores":{"kills":1}},
      {"championName":"오공","items":[],"rawChampionName": "game_character_displayname_MonkeyKing","riotId":"나#KR1","scores":{"kills":0}}
    ]"#;

    #[test]
    fn finds_my_champion() {
        assert_eq!(my_champion(LIST, "\"나#KR1\"\n"), Some("MonkeyKing"));
        assert_eq!(my_champion(LIST, "\"Other#KR1\""), Some("Ashe"));
        assert_eq!(my_champion(LIST, "\"Nobody#KR1\""), None);
        assert_eq!(my_champion(r#"{"errorCode":"RESOURCE_NOT_FOUND"}"#, "\"나#KR1\""), None);
        assert_eq!(my_champion(LIST, ""), None);
    }
}
