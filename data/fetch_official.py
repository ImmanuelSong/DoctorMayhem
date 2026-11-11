#!/usr/bin/env python3
"""게임 클라이언트 데이터(CommunityDragon)에서 증바람 증강의 공식 한국어 이름 목록을 받아
official_augments.json 으로 저장한다. 패치 후 다시 실행: python3 fetch_official.py

화면의 카드 제목은 이 공식 이름으로 나오므로, OCR 매칭은 이 목록을 기준으로 한다.
효과 설명(desc)도 클라이언트의 증바람 모드 데이터(kiwi.bin)와 문자열 표에서 가져와 수치를 채운다.
계산식으로만 나오는 수치(예: 피해량)는 채울 수 없어서 "?" 로 남는다.
"""
import json, re, urllib.request
from pathlib import Path

BASE = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/ko_kr/v1/"
GAME = "https://raw.communitydragon.org/latest/game/"
MODE = "KIWI"  # 증바람(ARAM: Mayhem)의 내부 모드 이름
RARITY = {"kSilver": "silver", "kGold": "gold", "kPrismatic": "prismatic"}


def get(name, base=BASE):
    req = urllib.request.Request(base + name, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def number(v):
    return f"{v:g}" if abs(v) >= 10 else f"{round(v, 2):g}"


def describe(aug, kiwi, strings, depth=0):
    """증강의 툴팁 문자열을 찾아 @값@ 을 스펠 데이터로 채우고 태그를 지운다."""
    text = strings.get(aug.get("AugmentTooltipTra", "").lower()) or strings.get(aug.get("DescriptionTra", "").lower(), "")
    spell = kiwi.get(aug.get("RootSpell"), {}).get("mSpell", {})
    values = {v["name"].lower(): v.get("values", [0]) for v in spell.get("DataValues", [])}

    def value(m):
        name, _, mult = m.group(1).partition("*")
        vs = values.get(name.lower())
        if vs is None:
            return "?"
        k = float(mult) if mult else 1.0
        lo, hi = min(vs[1:] or vs) * k, max(vs[1:] or vs) * k
        return number(lo) if lo == hi else f"{number(lo)}~{number(hi)}"

    def nested(m):
        name = m.group(1).strip()
        if name.lower() == "spellname" or depth > 2:
            return "선택한 스킬"
        return describe({"AugmentTooltipTra": name}, kiwi, strings, depth + 1) or ""

    text = re.sub(r"\{\{([^}]+)\}\}", nested, text)
    text = re.sub(r"@([A-Za-z0-9_.]+(?:\*[0-9.]+)?)@", value, text)
    text = re.sub(r"<br\s*/?>", " ", text)
    text = re.sub(r"%i:[^%]+%\s*", "", re.sub(r"<[^>]+>", "", text))
    text = re.split(r"-{6,}", text)[0]  # 그 아래는 게임 중 누적 통계
    text = re.sub(r"(챔피언에게 )?(입힌|가한) 피해량: \?|현재 [^.]*\?[^.]*\.?", "", text)
    return re.sub(r"\s+", " ", text).strip()


def key(s):
    return re.sub(r"[^a-z0-9]", "", s.lower())


if __name__ == "__main__":
    by_id = {key(a["augmentNameId"]): a for a in get("cherry-augments.json")}
    pool = next(l for l in get("augment-lists.json") if l["modeName"] == MODE)["augmentList"]
    kiwi = get("maps/modespecificdata/kiwi.bin.json", GAME)
    strings = get("ko_kr/data/menu/en_us/lol.stringtable.json", GAME)["entries"]
    defs = {key(v["AugmentNameId"]): v for v in kiwi.values() if isinstance(v, dict) and v.get("__type") == "AugmentData"}
    out = []
    for path in pool:
        a = by_id[key(path.rsplit("/", 1)[-1])]
        if a["nameTRA"].strip():
            desc = describe(defs.get(key(a["augmentNameId"]), {}), kiwi, strings)
            out.append({"id": a["augmentNameId"], "ko": a["nameTRA"].strip(), "rarity": RARITY.get(a["rarity"], a["rarity"]), "desc": desc})
    out.sort(key=lambda a: a["id"])
    dst = Path(__file__).parent / "official_augments.json"
    dst.write_text(json.dumps(out, ensure_ascii=False, indent=0) + "\n", encoding="utf-8")
    print(f"{dst}: {len(out)} augments, {sum(not a['desc'] for a in out)} without description")
