"""사용: python3 -I validate.py work/batchN.txt work/outN.json — 분석 결과 JSON 의 형식과 키를 검사한다."""
import json, re, sys
from pathlib import Path

here = Path(__file__).parent / "work"
augs = {l.split("|")[0]: l.split("|") for l in (here / "augments.txt").read_text().splitlines() if "|" in l and not l.startswith("#")}
items = {l.split("|")[0] for l in (here / "items.txt").read_text().splitlines() if "|" in l and not l.startswith("#")}
champs = re.findall(r"^## (\S+) \|", Path(sys.argv[1]).read_text(), re.M)
out = json.loads(Path(sys.argv[2]).read_text())
errs = []
for c in champs:
    if c not in out:
        errs.append(f"{c}: 없음")
for c, builds in out.items():
    if c not in champs:
        errs.append(f"{c}: 이 배치의 챔피언이 아님")
    if not 1 <= len(builds) <= 3:
        errs.append(f"{c}: 빌드 {len(builds)}개 (1~3)")
    for b in builds:
        if set(b) != {"name", "why", "augs", "items"}:
            errs.append(f"{c}: 필드 {sorted(b)}")
            continue
        if not 1 <= len(b["name"]) <= 14 or not 1 <= len(b["why"]) <= 60:
            errs.append(f"{c}/{b['name']}: name ≤14자, why ≤60자")
        if not 6 <= len(b["augs"]) <= 14 or len(set(b["augs"])) != len(b["augs"]):
            errs.append(f"{c}/{b['name']}: augs 6~14개, 중복 없이")
        if not 4 <= len(b["items"]) <= 6 or len(set(b["items"])) != len(b["items"]):
            errs.append(f"{c}/{b['name']}: items 4~6개, 중복 없이")
        errs += [f"{c}/{b['name']}: 모르는 증강 {a}" for a in b["augs"] if a not in augs]
        errs += [f"{c}/{b['name']}: 모르는 아이템 {i}" for i in b["items"] if i not in items]
        rar = {augs[a][2] for a in b["augs"] if a in augs}
        if len(rar) < 3:
            errs.append(f"{c}/{b['name']}: 프리즘·골드·실버가 모두 있어야 함 ({sorted(rar)})")
print("\n".join(errs) or f"ok: {len(out)} champions")
sys.exit(1 if errs else 0)
