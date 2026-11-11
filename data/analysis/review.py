#!/usr/bin/env python3
"""빌드 검토 앱: python3 data/analysis/review.py  → 브라우저에서 챔피언을 눌러 가며 빌드를 고치고 저장.

저장하면 builds.json 을 고치고 data/build_data.py 로 data.bin 을 다시 만든다.
고친 챔피언과 '확인' 누른 챔피언은 builds.json 의 reviewed 에 들어가고, 다음 패치 분석(merge.py)이 덮어쓰지 않는다.
자체 점검: python3 data/analysis/review.py --check
"""
import json, subprocess, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE.parent
sys.path.insert(0, str(DATA))
from build_data import ALIASES, norm_name  # noqa: E402

PORT = 8765


def read(p):
    return json.loads(p.read_text(encoding="utf-8"))


def load():
    d, b = read(DATA / "augment_data.json"), read(DATA / "builds.json")
    alias = {v: k for k, v in ALIASES.items()}
    desc = {norm_name(a["ko"]): a.get("desc", "") for a in read(DATA / "official_augments.json")}
    augs = {}
    for k, a in {**d["augments"], **b["augments_extra"]}.items():
        ko = alias.get(k, a["ko"])
        augs[k] = {"ko": ko, "rarity": a["rarity"], "desc": desc.get(norm_name(ko)) or a.get("summary", ""),
                   "sets": [s["ko"] for s in b["sets"] if k in s["augs"]]}
    items = {k: {"ko": v["ko"], "desc": ""} for k, v in {**d["items"], **b["items_extra"]}.items()}
    work = HERE / "work"  # prepare.py 가 만든 전체 완성템 목록·능력치(있으면)
    if (work / "items_catalog.json").exists():
        items = {k: {"ko": ko, "desc": ""} for k, ko in read(work / "items_catalog.json").items()} | items
    if (work / "items.txt").exists():
        for line in (work / "items.txt").read_text(encoding="utf-8").splitlines():
            k, _, rest = line.partition("|")
            if k in items and "|" in rest:
                items[k]["desc"] = rest.split("|", 1)[1]
    champs = {k: {"ko": c["ko"], "style": c.get("style", ""),
                  "stats": {e["aug"]: [e["grade"], e.get("short", "")] for r in ("prismatic", "gold", "silver") for e in c.get(r, [])}}
              for k, c in d["champions"].items()}
    return d, b, {"augs": augs, "items": items, "champs": champs, "builds": b["champions"], "reviewed": b.get("reviewed", [])}


def errors(payload, view):
    errs = []
    builds = payload.get("builds", {})
    errs += [f"{k}: 빠졌음" for k in view["champs"] if k not in builds]
    errs += [f"{k}: 없는 챔피언" for k in builds if k not in view["champs"]]
    for k, bs in builds.items():
        if not 1 <= len(bs) <= 3:
            errs.append(f"{k}: 빌드 {len(bs)}개 (1~3)")
        for i, x in enumerate(bs, 1):
            where = f"{view['champs'].get(k, {}).get('ko', k)} 빌드 {i}"
            if set(x) != {"name", "why", "augs", "items"} or not x["name"].strip():
                errs.append(f"{where}: 이름이 비었음")
                continue
            for field, known, what in (("augs", view["augs"], "증강"), ("items", view["items"], "아이템")):
                v = x[field]
                if not v:
                    errs.append(f"{where}: {what}이 비었음")
                errs += [f"{where}: 모르는 {what} {a}" for a in v if a not in known]
                if len(set(v)) != len(v):
                    errs.append(f"{where}: {what} 중복")
    return errs


def save(payload):
    d, b, view = load()
    errs = errors(payload, view)
    if errs:
        return {"ok": False, "msg": "\n".join(errs)}
    changed = [k for k in b["champions"] if payload["builds"][k] != b["champions"][k]]
    b["champions"] = {k: payload["builds"][k] for k in b["champions"]}
    b["reviewed"] = sorted(set(payload.get("reviewed", [])) & set(b["champions"]) | set(changed))
    used = {i for bs in b["champions"].values() for x in bs for i in x["items"]}
    order = [*b["items_extra"], *sorted(used - set(b["items_extra"]))]
    b["items_extra"] = {k: {"ko": view["items"][k]["ko"]} for k in order if k in used and k not in d["items"]}
    (DATA / "builds.json").write_text(json.dumps(b, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    r = subprocess.run([sys.executable, str(DATA / "build_data.py")], capture_output=True, text=True)
    if r.returncode:
        return {"ok": False, "msg": "builds.json 은 저장했지만 data.bin 만들기 실패:\n" + r.stderr[-2000:]}
    return {"ok": True, "msg": f"저장함. 바뀐 챔피언 {len(changed)}, 확인된 챔피언 {len(b['reviewed'])}", "reviewed": b["reviewed"]}


class Handler(BaseHTTPRequestHandler):
    def send(self, body, kind):
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/data":
            self.send(json.dumps(load()[2], ensure_ascii=False).encode(), "application/json")
        else:
            self.send((HERE / "review.html").read_bytes(), "text/html; charset=utf-8")

    def do_POST(self):
        if self.path != "/save" or self.headers.get("Origin") not in (None, f"http://localhost:{PORT}", f"http://127.0.0.1:{PORT}"):
            return self.send_error(403)
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.send(json.dumps(save(payload), ensure_ascii=False).encode(), "application/json")

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    if "--check" in sys.argv:
        _, b, view = load()
        assert errors({"builds": b["champions"]}, view) == []
        bad = json.loads(json.dumps(b["champions"]))
        bad["Ashe"][0]["augs"] += ["no_such_aug", bad["Ashe"][0]["augs"][0]]
        bad["Annie"] = []
        assert len(errors({"builds": bad}, view)) == 3, errors({"builds": bad}, view)
        print("ok")
        sys.exit()
    url = f"http://localhost:{PORT}/"
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"{url} 에서 열림. 끝내려면 Ctrl+C")
    for opener in (["cmd.exe", "/c", "start", url], ["xdg-open", url]):  # WSL 이면 Windows 브라우저
        try:
            subprocess.Popen(opener, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            break
        except FileNotFoundError:
            pass
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
