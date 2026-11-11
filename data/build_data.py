#!/usr/bin/env python3
"""augment_data.json -> data.bin (MYH1). 사용: python3 build_data.py [in.json] [out.bin]

레이아웃 (전부 little-endian, 문자열 오프셋은 풀 안의 UTF-16 코드 유닛 단위, 0 = 빈 문자열):
  header   20 x u32: magic "MYH1", version, n_aug, n_champ, n_entry, n_item, n_name,
                     off_pool, pool_units, off_aug, off_champ, off_entry, off_item, off_name,
                     patch_off, credit_off, n_build, off_build, off_list,
                     revision (입력 파일들의 가장 늦은 수정 시각, 유닉스 초 — 클수록 새 데이터)
  pool     UTF-16LE, \\0 구분
  augments 16B: key_hash u32, name_off u32, summary_off u32, rarity u8, flags u8, sets u16 (세트 비트마스크)
  champs   16B: key_hash u32, ko_off u32, entry_begin u32, entry_count u16, pad u16   (key_hash 오름차순)
  entries  28B: aug_idx u16, grade u8, flags u8, short_off u32, why_off u32, tip_off u32,
                pairs[3] u16, items[3] u16   (없으면 0xFFFF)
  items     4B: name_off u32
  names     8B: norm_off u32, aug_idx u32   (정규화 이름 오름차순. aug_idx 0xFFFFFFFF = 어디에도 없는 증강)
  builds   16B: champ_hash u32, name_off u32, why_off u32, list_off u16 (lists 안 u16 단위), n_aug u8, n_item u8
                (챔피언별로 주력 빌드부터)
  lists    u16 배열: 빌드마다 증강 번호들 뒤에 아이템 번호들

증강은 augment_data.json(등급 데이터 있음) 다음에 builds.json 의 augments_extra(게임에는 있지만 등급 데이터 없음),
아이템도 augment_data.json 다음에 builds.json 의 items_extra 순서.

builds.json 은 챔피언 스킬·증강 설명을 보고 만든 분석 데이터(빌드 방향 = 증강 묶음 + 아이템)다.
이름 목록은 official_augments.json(게임 클라이언트의 공식 이름, fetch_official.py 로 갱신)이 기준이고,
augment_data.json 의 이름(사이트 표기)도 함께 넣는다.
"""
import json, struct, sys
from pathlib import Path

VERSION = 2
NONE = 0xFFFF
NO_DATA = 0xFFFFFFFF
# 게임 화면의 이름(공식)과 augment_data.json 의 이름(사이트 표기)이 다른 증강: 공식 이름 -> 데이터 키
ALIASES = {
    "우글렛의 마녀 모자": "quest_wooglets",
    "강철 같은 심장": "steel_your_heart",
    "우르프의 챔피언": "quest_urf",
    "보강": "double_defense",
    "초장거리 저격": "from_downtown",  # 설명(스킬로 장거리 저격 퀘스트)으로 대응시킴. 게임에서 확인 필요
}
RARITY = {"silver": 0, "gold": 1, "prismatic": 2}
GRADE = {"A": 1, "S": 2}
AUG_RETIRED, AUG_VERIFY = 1, 2
ENT_VERIFY, ENT_COMBO = 1, 2
HEADER = struct.Struct("<4s19I")
BUILD = struct.Struct("<IIIHBB")
AUG = struct.Struct("<IIIBBH")
CHAMP = struct.Struct("<IIIHH")
ENTRY = struct.Struct("<HBBIII3H3H")


def norm_key(s):
    """챔피언/증강 키 정규화: 영숫자만, 소문자. 런타임과 반드시 같아야 함."""
    return "".join(c for c in s.lower() if c.isascii() and c.isalnum())


def norm_name(s):
    """OCR 매칭용 이름 정규화: 한글 음절 + ASCII 영숫자(소문자)만. 런타임과 반드시 같아야 함."""
    return "".join(c for c in s.lower() if "가" <= c <= "힣" or (c.isascii() and c.isalnum()))


def fnv1a(s):
    h = 0x811C9DC5
    for b in s.encode("ascii"):
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


class Pool:
    def __init__(self):
        self.units, self.seen = [0], {"": 0}

    def add(self, s):
        s = s or ""
        if s not in self.seen:
            self.seen[s] = len(self.units)
            raw = s.encode("utf-16-le")
            self.units += struct.unpack(f"<{len(raw) // 2}H", raw) + (0,)
        return self.seen[s]

    def bytes(self):
        return struct.pack(f"<{len(self.units)}H", *self.units)


def pad3(xs, index):
    idx = [index[x] for x in xs[:3]]
    return idx + [NONE] * (3 - len(idx))


def all_augments(d, extra):
    """등급 데이터가 있는 증강 다음에 없는 증강. 키 -> 증강 정보."""
    assert not set(d["augments"]) & set(extra)
    return {**d["augments"], **extra}


def name_index(d, official, extra=None):
    """정규화 이름 -> 증강 번호(또는 NO_DATA)."""
    augs = all_augments(d, extra or {})
    aug_idx = {k: i for i, k in enumerate(augs)}
    index = {norm_name(a["ko"]): aug_idx[k] for k, a in augs.items() if not a.get("retired")}
    assert len(index) == sum(not a.get("retired") for a in augs.values()), "정규화 이름 충돌"
    for name, key in ALIASES.items():
        assert not d["augments"][key].get("retired"), key
        index[norm_name(name)] = aug_idx[key]
    for a in official:
        index.setdefault(norm_name(a["ko"]), NO_DATA)
    return index


def build(d, official=(), b=None, revision=0):
    b = b or {"augments_extra": {}, "items_extra": {}, "sets": [], "champions": {}}
    pool = Pool()
    all_augs = all_augments(d, b["augments_extra"])
    all_items = {**d["items"], **b["items_extra"]}
    aug_idx = {k: i for i, k in enumerate(all_augs)}
    item_idx = {k: i for i, k in enumerate(all_items)}
    assert len(b["sets"]) <= 16
    sets = {k: 0 for k in all_augs}
    for n, st in enumerate(b["sets"]):
        for k in st["augs"]:
            sets[k] |= 1 << n

    augs = b"".join(
        AUG.pack(fnv1a(norm_key(k)), pool.add(a["ko"]), pool.add(a.get("summary")), RARITY[a["rarity"]],
                 (AUG_RETIRED if a.get("retired") else 0) | (AUG_VERIFY if a.get("verify") else 0), sets[k])
        for k, a in all_augs.items())
    items = b"".join(struct.pack("<I", pool.add(i["ko"])) for i in all_items.values())

    hashes = {fnv1a(norm_key(k)): k for k in d["champions"]}
    assert len(hashes) == len(d["champions"]), "챔피언 키 해시 충돌"
    champs, entries, n_entry = [], [], 0
    for h in sorted(hashes):
        c = d["champions"][hashes[h]]
        es = [e for r in ("prismatic", "gold", "silver") for e in c.get(r, [])]
        champs.append(CHAMP.pack(h, pool.add(c["ko"]), n_entry, len(es), 0))
        for e in es:
            flags = (ENT_VERIFY if e.get("verify") else 0) | (ENT_COMBO if e.get("pairs_basis") == "combo" else 0)
            entries.append(ENTRY.pack(aug_idx[e["aug"]], GRADE[e["grade"]], flags,
                                      pool.add(e["short"]), pool.add(e["why"]), pool.add(e.get("tip")),
                                      *pad3(e.get("pairs", []), aug_idx), *pad3(e.get("items", []), item_idx)))
        n_entry += len(es)

    norm = name_index(d, official, b["augments_extra"])
    names = b"".join(struct.pack("<II", pool.add(n), i) for n, i in sorted(norm.items()))

    builds, lists = [], []
    for ck, bs in b["champions"].items():
        assert ck in d["champions"], ck
        for x in bs:
            builds.append(BUILD.pack(fnv1a(norm_key(ck)), pool.add(x["name"]), pool.add(x["why"]), len(lists),
                                     len(x["augs"]), len(x["items"])))
            lists += [aug_idx[a] for a in x["augs"]] + [item_idx[i] for i in x["items"]]
    assert len(lists) < 0xFFFF

    patch_off, credit_off = pool.add(d["meta"]["patch"]), pool.add(d["meta"]["data_credit"])
    sections = [pool.bytes(), augs, b"".join(champs), b"".join(entries), items, names, b"".join(builds),
                struct.pack(f"<{len(lists)}H", *lists)]
    offs, pos = [], HEADER.size
    for s in sections:
        offs.append(pos)
        pos += len(s) + (-len(s) % 4)
    header = HEADER.pack(b"MYH1", VERSION, len(aug_idx), len(champs), n_entry, len(item_idx), len(norm),
                         offs[0], len(pool.units), *offs[1:6], patch_off, credit_off, len(builds), *offs[6:], revision)
    return header + b"".join(s + b"\0" * (-len(s) % 4) for s in sections)


def check(d, blob, b):
    """되읽어서 모든 (챔피언, 증강) 항목과 빌드가 조회되는지 확인."""
    (magic, ver, n_aug, n_champ, n_entry, n_item, n_name, off_pool, pool_units,
     off_aug, off_champ, off_entry, off_item, off_name, _, credit_off, n_build, off_build, off_list, _) = HEADER.unpack_from(blob)
    assert magic == b"MYH1" and ver == VERSION

    def s(off):
        assert off < pool_units
        end = off
        while blob[off_pool + end * 2: off_pool + end * 2 + 2] != b"\0\0":
            end += 1
        return blob[off_pool + off * 2: off_pool + end * 2].decode("utf-16-le")

    keys = list(d["augments"])
    champs = [CHAMP.unpack_from(blob, off_champ + i * CHAMP.size) for i in range(n_champ)]
    assert [c[0] for c in champs] == sorted(c[0] for c in champs)
    by_hash = {c[0]: c for c in champs}
    total = 0
    for ck, c in d["champions"].items():
        _, ko_off, begin, count, _ = by_hash[fnv1a(norm_key(ck))]
        assert s(ko_off) == c["ko"]
        got = {}
        for i in range(begin, begin + count):
            a, grade, flags, short, why, tip, *rest = ENTRY.unpack_from(blob, off_entry + i * ENTRY.size)
            got[keys[a]] = (grade, flags, s(short), s(why), s(tip), rest)
        for r in ("prismatic", "gold", "silver"):
            for e in c.get(r, []):
                grade, flags, short, why, tip, rest = got[e["aug"]]
                assert (grade, short, why, tip) == (GRADE[e["grade"]], e["short"], e["why"], e.get("tip") or "")
                assert bool(flags & ENT_VERIFY) == bool(e.get("verify"))
                assert [keys[p] for p in rest[:3] if p != NONE] == e["pairs"]
                assert [list(d["items"])[p] for p in rest[3:] if p != NONE] == e["items"]
                total += 1
    assert total == n_entry
    brand = by_hash[fnv1a("brand")]
    conduit = keys.index("infernal_conduit")
    assert any(ENTRY.unpack_from(blob, off_entry + i * ENTRY.size)[:2] == (conduit, GRADE["S"])
               for i in range(brand[2], brand[2] + brand[3])), "브랜드 지옥의 전도체가 S가 아님"
    names = [struct.unpack_from("<II", blob, off_name + i * 8) for i in range(n_name)]
    assert [s(n) for n, _ in names] == sorted(s(n) for n, _ in names)
    assert s(AUG.unpack_from(blob, off_aug + conduit * AUG.size)[1]) == "지옥의 전도체"
    assert s(credit_off) == d["meta"]["data_credit"]
    all_keys = keys + list(b["augments_extra"])
    all_items = list(d["items"]) + list(b["items_extra"])
    got = {}
    for i in range(n_build):
        h, name, why, lo, na, ni = BUILD.unpack_from(blob, off_build + i * BUILD.size)
        ids = struct.unpack_from(f"<{na + ni}H", blob, off_list + lo * 2)
        got.setdefault(h, []).append({"name": s(name), "why": s(why), "augs": [all_keys[a] for a in ids[:na]],
                                      "items": [all_items[x] for x in ids[na:]]})
    assert got == {fnv1a(norm_key(k)): v for k, v in b["champions"].items()}
    return n_aug, n_champ, n_entry, n_item, n_name, n_build


if __name__ == "__main__":
    here = Path(__file__).parent
    src = Path(sys.argv[1]) if len(sys.argv) > 1 else here / "augment_data.json"
    dst = Path(sys.argv[2]) if len(sys.argv) > 2 else here / "data.bin"
    data = json.loads(src.read_text(encoding="utf-8"))
    official = json.loads((here / "official_augments.json").read_text(encoding="utf-8"))
    builds = json.loads((here / "builds.json").read_text(encoding="utf-8"))
    inputs = [src, here / "official_augments.json", here / "builds.json"]
    out = build(data, official, builds, int(max(p.stat().st_mtime for p in inputs)))
    index = name_index(data, official, builds["augments_extra"])
    assert index[norm_name("보강")] == list(data["augments"]).index("double_defense")
    assert index[norm_name("빵과 잼")] >= len(data["augments"]) and index[norm_name("빵과 버터")] < len(data["augments"])
    assert all(norm_name(a["ko"]) in index for a in official), "공식 증강 중 이름 색인에 없는 것이 있음"
    print(f"names: {len(index)} ({sum(v >= len(data['augments']) for v in index.values())} without grade data)")
    counts = check(data, out, builds)
    dst.write_bytes(out)
    print(f"{dst}: {len(out):,} bytes, augments/champions/entries/items/names/builds = {counts}")
