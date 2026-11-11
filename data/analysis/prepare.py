#!/usr/bin/env python3
"""패치 후 빌드 분석 입력을 만든다: python3 data/analysis/prepare.py  (먼저 data/fetch_official.py)

work/ 에 쓴다:
  augments.txt  공식 증강 241개(key|이름|등급|세트|설명) + 세트 설명
  items.txt     완성 아이템(key|이름|가격·능력치·고유효과)
  batchN.txt    챔피언 20명씩: 스킬, 통계 등급, 자주 가는 아이템, 사이트 조합 평가
  catalog.json, items_catalog.json  merge.py 가 쓰는 원본
그다음 BRIEF.md 대로 배치마다 분석 → work/outN.json → validate.py → merge.py → data/build_data.py
"""
import json, re, time, urllib.request
from pathlib import Path

HERE = Path(__file__).parent
DATA = HERE.parent
WORK = HERE / "work"
CDRAGON = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/ko_kr/v1/"
SITE_INDEX = "https://arammayhem.com/search-index/ko-kr.json"  # 세트·조합 평가·아이템 목록
ALIASES = {"우글렛의 마녀 모자": "quest_wooglets", "강철 같은 심장": "steel_your_heart", "우르프의 챔피언": "quest_urf",
           "보강": "double_defense", "초장거리 저격": "from_downtown"}  # data/build_data.py 의 ALIASES 와 같게
BATCH = 20


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def n(t):
    return re.sub(r"[^가-힣a-z0-9]", "", t.lower())


def strip(t):
    return re.sub(r"\s+", " ", re.sub(r"<[^>]+>", " ", t or "")).strip()


def catalog(d, official, site):
    dko = {n(a["ko"]): k for k, a in d["augments"].items()}
    site_ko = {n(a["name"]["ko-KR"]): a for a in site["augments"]}
    site_en = {n(a["name"]["en"]): a for a in site["augments"]}
    cat = []
    for a in official:
        key = ALIASES.get(a["ko"]) or dko.get(n(a["ko"]))
        sa = site_ko.get(n(a["ko"])) or (site_en.get(n(d["augments"][key]["en"])) if key else None)
        if not key:
            key = sa["id"] if sa else re.sub(r"^aram_", "", re.sub(r"(?<=[a-z])(?=[A-Z])", "_", a["id"]).lower())
        en = (d["augments"].get(key) or {}).get("en") or (sa["name"]["en"] if sa else "")
        cat.append(dict(key=key, ko=a["ko"], en=en, rarity=a["rarity"], desc=a.get("desc", ""), graded=key in d["augments"]))
    assert len({c["key"] for c in cat}) == len(cat), "증강 키 중복"
    by_en = {n(c["en"]): c["key"] for c in cat if c["en"]}
    sets = [dict(id=x["id"], ko=x["name"]["ko-KR"], desc=x["description"]["ko-KR"],
                 augs=[by_en[n(a)] for a in x["augments"] if n(a) in by_en]) for x in site["synergySets"]]
    for c in cat:
        c["sets"] = [x["ko"] for x in sets if c["key"] in x["augs"]]
    site2key = {a["id"]: (dko.get(n(a["name"]["ko-KR"])) or by_en.get(n(a["name"]["en"])) or
                          next((c["key"] for c in cat if n(c["ko"]) == n(a["name"]["ko-KR"])), None)) for a in site["augments"]}
    return cat, sets, site2key


def items(d, site, cd_items):
    cd = {}
    for i in cd_items:
        if i["priceTotal"] >= 900:
            cd.setdefault(n(i["name"]), i)
    out, seen = {}, set()
    for k, v in d["items"].items():
        out[k] = v["ko"]
        seen.add(n(v["ko"]))
    for i in site["items"]:
        ko = i["name"]["ko-KR"]
        if n(ko) not in seen:
            out[i["id"]] = ko
            seen.add(n(ko))
    lines = []
    for k, ko in out.items():
        i = cd.get(n(ko))
        if not i:
            continue  # 재료·소모품
        st = re.search(r"<stats>(.*?)</stats>", i["description"], re.S)
        effects = [re.sub("<[^>]+>", "", a or b) for a, b in re.findall(r"<passive>(.*?)</passive>|<active>(.*?)</active>", i["description"])]
        stats = strip(st.group(1)) if st else ""
        lines.append(f"{k}|{ko}|{i['priceTotal']}G {stats}" + (f" / {', '.join(effects)}" if effects else ""))
    return out, lines


def champion_text(k, c, kit, combos, site2key):
    lines = [f"## {k} | {c['ko']} | 픽률 순위 {c.get('pick_rank')}", f"플레이: {c.get('style', '')}",
             f"P {kit['passive']['name']}: {strip(kit['passive']['description'])}"]
    lines += [f"{s['spellKey'].upper()} {s['name']}: {strip(s['description'])}" for s in kit["spells"]]
    lines.append("자주 가는 완성템(통계): " + ", ".join(c.get("core_items", [])))
    for r in ("prismatic", "gold", "silver"):
        lines.append(f"{r} 등급(통계 기반): " + "; ".join(f"{e['aug']}={e['grade']}({e.get('pick_rank')}위)" for e in c.get(r, [])))
    cc = [x for x in combos if x["championId"].lower() == k.lower()]
    if cc:
        lines.append("사이트 조합 평가: " + "; ".join(
            "+".join(site2key.get(a) or "?" + a for a in x["augmentIds"]) + "=" + x["tier"] for x in cc))
    return "\n".join(lines)


if __name__ == "__main__":
    WORK.mkdir(exist_ok=True)
    d = json.loads((DATA / "augment_data.json").read_text(encoding="utf-8"))
    official = json.loads((DATA / "official_augments.json").read_text(encoding="utf-8"))
    site = get(SITE_INDEX)
    cat, sets, site2key = catalog(d, official, site)
    with open(WORK / "augments.txt", "w", encoding="utf-8") as f:
        f.write("# key|이름|등급|세트|설명 (게임 클라이언트 공식 설명, ? = 계산식이라 못 채운 수치)\n")
        for r in ("prismatic", "gold", "silver"):
            f.writelines(f"{c['key']}|{c['ko']}|{r}|{','.join(c['sets'])}|{c['desc'] or '설명 없음'}\n" for c in cat if c["rarity"] == r)
        f.write("\n# 세트 (같은 세트 증강을 여러 개 모으면 세트 보너스)\n")
        f.writelines(f"{x['ko']}: {x['desc']} — {', '.join(x['augs'])}\n" for x in sets)
    (WORK / "catalog.json").write_text(json.dumps(dict(augments=cat, sets=sets), ensure_ascii=False), encoding="utf-8")

    all_items, item_lines = items(d, site, get(CDRAGON + "items.json"))
    (WORK / "items.txt").write_text("# key|이름|가격·능력치·고유효과 이름\n" + "\n".join(item_lines) + "\n", encoding="utf-8")
    (WORK / "items_catalog.json").write_text(json.dumps(all_items, ensure_ascii=False), encoding="utf-8")

    ids = {re.sub(r"[^a-z0-9]", "", c["alias"].lower()): c["id"] for c in get(CDRAGON + "champion-summary.json")}
    texts = []
    for k, c in d["champions"].items():
        kit = get(CDRAGON + f"champions/{ids[re.sub(r'[^a-z0-9]', '', k.lower())]}.json")
        texts.append(champion_text(k, c, kit, site["combos"], site2key))
        time.sleep(0.05)
    for old in WORK.glob("batch*.txt"):
        old.unlink()
    for b in range(0, len(texts), BATCH):
        (WORK / f"batch{b // BATCH + 1}.txt").write_text("\n\n".join(texts[b:b + BATCH]) + "\n", encoding="utf-8")
    print(f"{WORK}: augments {len(cat)}, items {len(item_lines)}, champions {len(texts)} in {-(-len(texts) // BATCH)} batches")
