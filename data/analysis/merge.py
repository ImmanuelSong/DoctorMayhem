#!/usr/bin/env python3
"""work/outN.json 들 → data/builds.json. 그다음 python3 data/build_data.py"""
import json
from pathlib import Path

HERE = Path(__file__).parent
WORK = HERE / "work"
DATA = HERE.parent

d = json.loads((DATA / "augment_data.json").read_text(encoding="utf-8"))
cat = json.loads((WORK / "catalog.json").read_text(encoding="utf-8"))
items = json.loads((WORK / "items_catalog.json").read_text(encoding="utf-8"))
champions = {}
for p in sorted(WORK.glob("out*.json")):
    champions.update(json.loads(p.read_text(encoding="utf-8")))
old = json.loads((DATA / "builds.json").read_text(encoding="utf-8"))
reviewed = [k for k in old.get("reviewed", []) if k in d["champions"]]
keys = {a["key"] for a in cat["augments"]}
for k in reviewed:  # 사람이 검토한 빌드는 분석 결과로 덮지 않는다. 패치로 사라진 증강·아이템만 뺀다.
    for b in old["champions"][k]:
        gone = [a for a in b["augs"] if a not in keys] + [i for i in b["items"] if i not in items]
        if gone:
            print(f"{k}/{b['name']}: 이번 패치에 없어서 뺌 {gone}")
        b["augs"] = [a for a in b["augs"] if a in keys]
        b["items"] = [i for i in b["items"] if i in items]
    champions[k] = old["champions"][k]
missing = [k for k in d["champions"] if k not in champions]
assert not missing, f"분석 안 된 챔피언: {missing}"
used_items = {i for bs in champions.values() for b in bs for i in b["items"]}
out = {
    "note": "챔피언 스킬·증강 설명·통계 등급을 보고 만든 분석 데이터(통계 아님). 빌드 = 서로 시너지가 나는 증강 묶음 + 아이템, 첫 빌드가 주력.",
    "augments_extra": {a["key"]: {"ko": a["ko"], "rarity": a["rarity"], "summary": a["desc"]}
                       for a in cat["augments"] if a["key"] not in d["augments"]},
    "items_extra": {k: {"ko": ko} for k, ko in items.items() if k not in d["items"] and k in used_items},
    "sets": [{"id": s["id"], "ko": s["ko"], "augs": s["augs"]} for s in cat["sets"]],
    "champions": {k: champions[k] for k in d["champions"]},
    "reviewed": reviewed,
}
(DATA / "builds.json").write_text(json.dumps(out, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
print(f"champions {len(out['champions'])}, extra augments {len(out['augments_extra'])}, extra items {len(out['items_extra'])}")
