# 증바람 증강 오버레이 — 개발 명세서

> 이 문서는 IDE(예: Claude Code, VS Code, Visual Studio)에서 그대로 작업 지시서로 쓰기 위한 것입니다.
> 데이터(`augment_data.json`)는 이미 완성되어 있고, 이 문서는 **Windows 오버레이 프로그램**을 만드는 방법을 정의합니다.
> 최우선 원칙: **가볍게, 메모리 적게, 필요할 때만 일하기.**

---

## 1. 한 줄 요약

증바람(ARAM: Mayhem)에서 **증강 선택 화면이 뜰 때만** 각 카드 위에 **S/A 등급 + 한 줄 이유**를 띄우고, 마우스를 올린 카드의 **상세 설명·궁합 증강·자주 가는 아이템·운영 팁**을 옆 패널에 보여주는 투명 오버레이.

---

## 2. 확정된 요구사항 (지금까지 대화에서 정한 것)

| 항목 | 결정 |
|---|---|
| 등급 | **S / A 두 단계만.** 1·2·3순위 매기지 않음. S/A에 없는 증강은 표시 안 함(또는 "-") |
| 등급 기준 | 챔피언별 **정적** 데이터(선택률 순위 + 조합 평가 + 사용자 고정값). 게임 상황(상대 조합·내 아이템)으로 바꾸지 않음 |
| 승률 | **표시 금지** (Riot 정책: 증강 승률 노출 불가) |
| 카드 위 표시 | 등급 배지(S/A) + `short` 한 줄 |
| 옆 패널 | 마우스를 올린 카드의 `why`, 궁합 증강(`pairs`), 아이템(`items`), 팁(`tip`) |
| 패널 전환 | 마우스 위치(`GetCursorPos`) 폴링으로 호버 카드 판단. 오버레이는 **클릭 통과**(click-through) |
| 리롤 | 카드가 바뀌면 다시 인식해서 다시 표시 |
| 표시 시점 | **증강 선택 화면이 열려 있을 때만.** 선택이 끝나면 즉시 숨김 |
| 가림 허용 | 선택 화면에서만 뜨므로 게임 알림 일부를 가려도 됨 |
| 고정값 | 브랜드 `infernal_conduit`(지옥의 전도체)은 항상 S |
| 대상 OS | Windows 10/11 |
| 안전 | 게임 메모리 읽기·DLL 주입·DirectX 후킹·입력 자동화 **전부 금지**. 별도 창에 그리기만 함 |

---

## 3. Riot 정책·안티치트(Vanguard) 가드레일

**이 규칙을 어기는 기능은 만들지 않는다.**

- ❌ 게임 프로세스 메모리 읽기/쓰기, DLL 인젝션, 그래픽 API 후킹(오버레이를 게임 안에 그리기)
- ❌ 키 입력·마우스 클릭 자동화(증강 자동 선택 등)
- ❌ 증강 **승률** 표시, 게임 상황 기반 "지금 이걸 골라라" 식의 동적 추천
- ✅ 화면 캡처(OBS처럼 화면에 보이는 것을 읽는 것)
- ✅ Riot 공식 로컬 API **Live Client Data API**(`https://127.0.0.1:2999/liveclientdata/...`) 읽기 — 내 챔피언 확인용
- ✅ 게임 창 위에 뜨는 **별도의 투명 최상위 창**(Blitz·Porofessor 같은 방식)
- ✅ 미리 만들어 둔 **정적** 등급·설명 표시

> 배포(친구에게 공유 등) 전에 Riot Developer Portal에 제품 등록을 검토할 것. 개인 사용 단계에서는 로컬 API만 쓰므로 API 키가 필요 없다.

---

## 4. 기술 스택 (가벼움 우선)

### 권장: **C++20 + Win32 API + Direct2D/DirectWrite**
- 런타임 설치 불필요, 단일 `.exe`(정적 링크) 목표 **1~3 MB**
- 유휴 시 메모리 목표 **10 MB 이하**, CPU **0.1% 이하**
- 그리기: `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE` 창 + Direct2D(또는 `UpdateLayeredWindow` + 32bit DIB)
- JSON: **yyjson**(C 단일 파일, 빠르고 메모리 적음) — 또는 빌드 단계에서 바이너리로 미리 변환(아래 6장)
- OCR(카드 이름 읽기): **Windows 내장 OCR**(`Windows.Media.Ocr`, C++/WinRT) — 추가 모델 파일 불필요, 한국어 지원(한국어 언어팩 필요)

### 대안 (참고)
| 스택 | 장점 | 단점 |
|---|---|---|
| Rust + `windows` crate | 메모리 안전, 단일 exe 작음 | 빌드 환경 학습 필요 |
| C# .NET 8 WinForms | 개발 빠름 | 런타임 포함 시 용량↑, 메모리 30~60 MB |
| Electron / WebView | UI 쉬움 | **메모리 100 MB+ → 이 프로젝트에 부적합** |

---

## 5. 전체 구조

```
mayhem-overlay/
├─ data/
│  ├─ augment_data.json        # 원본 데이터 (zip에 포함된 것)
│  └─ build_data.py            # json → data.bin 변환 (빌드 시 1회)
├─ src/
│  ├─ main.cpp                 # 진입점, 메시지 루프, 상태 머신
│  ├─ game_watch.cpp/.h        # 게임 창 찾기, 포그라운드 체크
│  ├─ live_client.cpp/.h       # 127.0.0.1:2999 에서 내 챔피언 이름 1회 조회
│  ├─ screen_detect.cpp/.h     # 증강 선택 화면 열림/닫힘 판별 (픽셀 샘플)
│  ├─ card_reader.cpp/.h       # 카드 3장 이름 인식 (dHash 캐시 → OCR 폴백)
│  ├─ data_store.cpp/.h        # data.bin 로드, (챔피언, 증강) → 항목 조회
│  ├─ overlay.cpp/.h           # 투명 창, Direct2D 그리기, 클릭 통과
│  ├─ hover.cpp/.h             # GetCursorPos 폴링 → 호버 카드 인덱스
│  ├─ layout.cpp/.h            # 해상도별 카드 좌표 계산 + 보정값
│  └─ config.cpp/.h            # settings.ini (보정값, 단축키, 투명도)
├─ third_party/yyjson/
├─ settings.ini
└─ CMakeLists.txt
```

### 상태 머신 (main.cpp)

```
IDLE ──(게임 창 발견)──▶ IN_GAME ──(선택 화면 감지)──▶ SELECTING ──(화면 닫힘)──▶ IN_GAME
  ▲                         │                              │
  └──────(게임 창 사라짐)────┘                              └─(카드 변경=리롤)─▶ 다시 인식
```

| 상태 | 하는 일 | 폴링 주기 |
|---|---|---|
| IDLE | `FindWindow`로 롤 게임 창만 확인 | 2초 |
| IN_GAME | 챔피언 이름 1회 조회 후 캐시, 선택 화면 픽셀 샘플 | 300 ms |
| SELECTING | 오버레이 표시, 호버 판정, 카드 변경 감지 | 호버 33 ms(30Hz), 카드 변경 300 ms |

- **게임 창이 포그라운드가 아니면** 모든 캡처를 멈춘다.
- 타이머는 `SetTimer` 하나 또는 `MsgWaitForMultipleObjects`로. 바쁜 루프 금지.

---

## 6. 데이터

### 6.1 원본: `augment_data.json` (zip에 포함, 현재 v8)

```jsonc
{
  "meta": { "patch": "26.22", "data_credit": "Data: arammayhem.com (CC BY 4.0)", ... },
  "items":    { "infinity_edge": { "ko": "무한의 대검", "en": "Infinity Edge" }, ... },
  "augments": { "infernal_conduit": { "ko": "지옥의 전도체", "en": "Infernal Conduit",
                 "rarity": "prismatic", "summary": "...", "tags": [...] }, ... },
  "champions": {
    "Brand": {
      "ko": "브랜드", "style": "...", "core_items": ["liandry", ...],
      "pick_rank": 11, "pick_rate": 11.99,
      "prismatic": [ { "aug": "infernal_conduit", "grade": "S",
                       "short": "...", "why": "...", "tip": "...(선택)",
                       "pairs": ["warlock_juicebox"], "pairs_basis": "combo|top",
                       "items": ["liandry"], "src": "...", "verify": true(선택) } ],
      "gold": [...], "silver": [...]
    }
  }
}
```

- 챔피언 173명, 항목 3,087개, 증강 176종(삭제된 `rabble_rousing`, `dropbear`는 `retired: true`)
- `pairs_basis`: `combo` = 사이트 조합 데이터 근거 → "같이 먹으면 좋은 증강", `top` = 근거 없음 → "이 챔피언의 다른 S 증강"으로 라벨을 다르게 표시
- `verify: true` 항목은 패널에 작은 "검증 필요" 표시

### 6.2 런타임용: `data.bin` (빌드 시 변환, 권장)

JSON을 실행 중에 파싱하지 않고, `build_data.py`로 미리 **문자열 풀 + 고정 크기 레코드** 바이너리로 바꾼다.

```
[header] magic "MYH1", version, counts
[string pool] UTF-16LE, \0 구분 (모든 ko 문자열)
[augments]   key_hash(u32), name_off(u32), rarity(u8), summary_off(u32)
[champions]  key_hash(u32), ko_off(u32), entry_begin(u32), entry_count(u16)
[entries]    aug_idx(u16), grade(u8), flags(u8: verify, pairs_basis),
             short_off, why_off, tip_off, pairs[3](u16), items[3](u16)
[name index] 정규화된 한국어 증강 이름 → aug_idx (OCR 매칭용, 정렬 배열)
```

- `MapViewOfFile`로 읽기만 → 파싱 비용 0, 메모리 추가 할당 거의 없음(약 0.5~1 MB)
- 조회: 챔피언 key → 이진 탐색 → 그 챔피언 항목만 선형 탐색(최대 ~20개)

---

## 7. 핵심 모듈 상세

### 7.1 내 챔피언 알아내기 — `live_client`
- 게임이 시작된 뒤 `GET https://127.0.0.1:2999/liveclientdata/activeplayer` 또는 `/playerlist`에서 내 `championName`(영문 키)을 가져온다.
- 인증서가 자체 서명이므로 WinHTTP에서 `SECURITY_FLAG_IGNORE_UNKNOWN_CA` 등 무시 플래그 사용 (로컬 127.0.0.1 한정).
- 게임당 1회 성공하면 캐시. 실패 시 5초마다 재시도.
- 영문 키가 데이터 키와 다른 경우 매핑: `MonkeyKing`(오공), `Kaisa`, `KSante`, `Belveth` 등 → 공백·특수문자 제거 후 비교.

### 7.2 선택 화면 감지 — `screen_detect`
- 게임 창 클라이언트 영역에서 **작은 영역 몇 군데만** `BitBlt`로 캡처(전체 화면 캡처 금지).
- 카드 3장 테두리/배경의 대표 픽셀 6~9개를 샘플링해 "카드가 떠 있음" 패턴과 비교(색 거리 임계값).
- 연속 2회 일치하면 SELECTING 진입, 연속 2회 불일치면 종료(깜빡임 방지).
- **창 모드 또는 테두리 없는 창 모드 필요.** 전체 화면(독점)에서는 GDI 캡처가 검게 나올 수 있음 → 설정 안내.
- 필요하면 `Windows.Graphics.Capture`로 교체 가능(창 단위 캡처, 더 안정적이나 약간 무거움).

### 7.3 카드 인식 — `card_reader`
두 단계로 가볍게:

1. **빠른 경로 (dHash 캐시)**: 각 카드의 아이콘 영역(예: 64×64)을 9×8 그레이로 줄여 64bit dHash 계산 → 로컬 캐시(`icon_cache.bin`)에서 해밍 거리 ≤ 6이면 즉시 확정. 비용: 마이크로초 단위.
2. **느린 경로 (OCR 폴백)**: 캐시에 없으면 카드 **제목 영역만** 잘라 `Windows.Media.Ocr`(언어 `ko`)로 읽고, 정규화(공백·특수문자 제거) 후 증강 이름 목록과 **편집 거리** 최소값으로 매칭. 확정되면 그 아이콘 dHash를 캐시에 저장 → 다음부터는 1번으로 처리.

- OCR 엔진은 처음 필요할 때 생성하고, 선택 화면이 닫히면 해제(메모리 반환).
- 같은 증강이 3장 중 2장 이상 나오는 경우는 없으므로 중복이면 재시도.
- 인식 실패 카드는 배지에 "?"만 표시하고 패널은 비움.

### 7.4 카드 좌표 — `layout`
- 테스트 스크린샷(흐웨이, 1920×1080 기준)에서 맞춘 비율값(클라이언트 영역 대비 %):
  - 가운데 카드 중심 x = **50.2%**, 카드 중심 간격 = **19.2%**, 카드 폭 = **16.5%**
  - 카드 위쪽 y = **18%**, 카드 높이 = **48.5%**
- 해상도·비율(16:9 / 16:10 / 21:9)이 다르면 어긋날 수 있으므로 `settings.ini`에서 보정값 수정 가능하게.
- **보정 모드**(단축키): 오버레이에 카드 테두리 가이드를 그려서 사용자가 화살표 키로 맞추고 저장.

### 7.5 오버레이 그리기 — `overlay`
- 게임 창의 클라이언트 영역 위치·크기를 따라가는 투명 창 1개.
- 클릭 통과: `WS_EX_TRANSPARENT` + `WS_EX_LAYERED`. 포커스 뺏지 않기: `WS_EX_NOACTIVATE`. 작업 표시줄 숨김: `WS_EX_TOOLWINDOW`.
- 그리는 것:
  - **카드 위 배지**: S(주황)/A(파랑) 원형 배지 + `short` 한 줄(반투명 검정 바탕)
  - **옆 패널**: 호버 중인 카드 기준. 증강 이름, 등급, `why`, 궁합 증강 칩(라벨은 `pairs_basis`에 따라), 아이템 칩, `tip`, "검증 필요" 표시
  - 호버가 없으면 패널은 등급이 가장 높은 카드(S가 여럿이면 가장 왼쪽)를 기본 표시
- **다시 그리기는 상태가 바뀔 때만**(호버 카드 변경, 카드 변경). 매 프레임 그리지 않음.
- 폰트: `맑은 고딕`(DirectWrite), 크기는 클라이언트 높이에 비례.

### 7.6 호버 — `hover`
- SELECTING 상태에서만 30Hz로 `GetCursorPos` → 클라이언트 좌표 변환 → 카드 사각형 3개와 비교.
- 인덱스가 바뀔 때만 overlay에 다시 그리기 요청.

### 7.7 단축키·설정
| 키 | 동작 |
|---|---|
| `` ` `` (백틱) | 오버레이 일시 숨김/보이기 |
| `Ctrl+Alt+F9` | 보정 모드 |
| `Ctrl+Alt+F10` | 프로그램 종료 |

- `RegisterHotKey` 사용(키 후킹 금지).
- `settings.ini`: 카드 비율값, 패널 위치(오른쪽/왼쪽), 투명도, 글자 크기.

---

## 8. 성능 예산 (지켜야 할 숫자)

| 항목 | 목표 |
|---|---|
| 실행 파일 | ≤ 3 MB |
| 메모리(Working Set) 유휴 | ≤ 10 MB |
| 메모리 선택 화면 중(OCR 포함) | ≤ 40 MB, 닫히면 다시 ≤ 12 MB |
| CPU 유휴/게임 중 | ≤ 0.1% / ≤ 0.5% |
| 선택 화면 감지 → 배지 표시 | ≤ 300 ms (캐시 적중 시 ≤ 100 ms) |
| 호버 반응 | ≤ 50 ms |

체크 방법: 작업 관리자 "세부 정보" 탭의 메모리(전용 작업 집합), Windows Performance Recorder로 CPU 확인.

---

## 9. 구현 순서 (마일스톤)

1. **M1 — 오버레이 뼈대**: 투명·클릭 통과 창을 게임 창에 붙이고, 고정 좌표에 가짜 배지 3개를 그린다. 단축키로 숨김/종료.
2. **M2 — 데이터**: `build_data.py`로 `data.bin` 생성, `data_store`로 (챔피언, 증강) 조회. 콘솔 테스트.
3. **M3 — 챔피언 확인**: Live Client Data API에서 내 챔피언 가져오기.
4. **M4 — 선택 화면 감지**: 픽셀 샘플 방식. 스크린샷 파일을 넣어 오프라인 테스트할 수 있게 `--image path.png` 옵션 제공.
5. **M5 — 카드 인식**: OCR 폴백 먼저 → dHash 캐시 추가.
6. **M6 — 패널 + 호버**: 상세 패널, 호버 전환, 리롤 감지.
7. **M7 — 보정 모드·설정 파일**.
8. **M8 — 성능 측정·정리**: 8장 예산 확인, 메모리 누수 점검.

각 마일스톤 끝에 실제 게임(또는 스크린샷)으로 확인.

---

## 10. 테스트

- **오프라인**: 증강 선택 화면 스크린샷 여러 장(해상도별·리롤 전후)을 `tests/screens/`에 두고 `--image`로 감지·인식 결과를 콘솔 출력.
- **데이터**: 모든 챔피언·증강 키가 `data.bin`에서 조회되는지, 브랜드 지옥의 전도체가 S인지 확인하는 단위 테스트.
- **실게임**: 증바람 몇 판 → 선택 화면에서만 뜨는지, 리롤 시 바뀌는지, 선택 후 바로 사라지는지, 게임 프레임 저하가 없는지.

---

## 11. 아직 정하지 않은 것 / 확인 필요

- [ ] 증강 선택 화면의 정확한 픽셀 샘플 위치·색 (실제 스크린샷 여러 장으로 결정)
- [ ] 카드 제목 영역 좌표 (OCR 정확도에 직결)
- [ ] 21:9·4K에서 카드 비율값이 유지되는지
- [ ] `이케시아의 몰락`(void_immolation): 사이트에는 삭제 표시가 있지만 공식 패치노트에서 확인되지 않음 → 게임에서 확인
- [ ] 패치마다 데이터 갱신 방법 (zip 안의 스크립트로 재생성 → `data.bin`만 교체)

---

## 12. 배포와 "안전하지 않은 파일" 경고

| 상황 | 경고 | 해결 |
|---|---|---|
| 내 PC에서 직접 빌드한 exe 실행 | **안 뜸** (인터넷에서 받은 표시(Mark of the Web)가 없음) | 그대로 사용 |
| 받은 exe/zip 실행(나) | 브라우저 경고 + SmartScreen | 파일 우클릭 → 속성 → **차단 해제** 체크, 또는 SmartScreen "추가 정보 → 실행" |
| 친구에게 배포 | SmartScreen 경고 | 코드 서명 + 평판 누적. 서명해도 처음엔 경고가 남을 수 있음 |

- Windows 11 **Smart App Control**이 켜진 PC는 서명 없는 exe 실행을 막을 수 있음.
- SmartScreen 자체를 끄는 방식은 권장하지 않음.

---

## 13. 데이터 출처·표기

- 증강 설명·선택률·조합: **Data: arammayhem.com (CC BY 4.0)** — 프로그램 정보 창에 표기
- 증강 삭제/재활성 여부: League of Legends 공식 패치노트로 확인
- 승률 수치는 데이터에 넣지 않았고 화면에도 표시하지 않음
