#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""用 GitHub git-data API 推送本地 commit（直连 github.com 不通时的应急路径）。

前提：本机 git 仓库里已经有完整 commit（01cf20b），只是推不上去。
做法：把改动文件逐个 POST 成 blob → 建 tree → 建 commit → 更新 ref。
"""
import base64
import json
import os
import subprocess
import sys

REPO = "lymsiq/IPRC1000-Native"
ROOT = r"F:/vibe/iprc1000_native"
PARENT = "c01748d0a9f62f937aaa8fb63292c5248a877b85"  # 远程 main（0.4.67），git-data API 要完整 SHA
TAG = "v0.4.68"
MSG = subprocess.run(["git", "-C", ROOT, "log", "-1", "--pretty=%B"],
                     capture_output=True, text=True, encoding="utf-8").stdout.strip()


def token():
    out = subprocess.run(["git", "credential", "fill"], input="protocol=https\nhost=github.com\n\n",
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.startswith("password="):
            return line.split("=", 1)[1]
    raise SystemExit("no token")


TOKEN = token()
PROXY = os.environ.get("https_proxy") or os.environ.get("HTTPS_PROXY") or ""


def api(method, path, payload=None):
    url = path if path.startswith("http") else f"https://api.github.com/repos/{REPO}{path}"
    cmd = ["curl", "-sS", "-X", method, "-H", f"Authorization: token {TOKEN}",
           "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: iprc-push"]
    if PROXY:
        cmd += ["--proxy", PROXY]
    tmp = None
    if payload is not None:
        tmp = os.path.join(ROOT, "_api_payload.json")
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(payload, f, ensure_ascii=False)
        cmd += ["--data-binary", "@" + tmp, "-H", "Content-Type: application/json"]
    cmd.append(url)
    r = subprocess.run(cmd, capture_output=True, text=True)
    if tmp and os.path.exists(tmp):
        os.remove(tmp)
    try:
        return json.loads(r.stdout)
    except Exception:
        raise SystemExit(f"bad response for {method} {path}: {r.stdout[:400]} {r.stderr[:200]}")


# 1) 父 commit 的 tree
parent = api("GET", f"/git/commits/{PARENT}")
if "tree" not in parent:
    raise SystemExit(f"parent lookup failed: {parent}")
base_tree = parent["tree"]["sha"]
print("parent tree", base_tree[:12])

# 2) 改动文件 → blob
names = subprocess.run(["git", "-C", ROOT, "-c", "core.quotepath=false", "diff", "--name-only", PARENT, "HEAD"],
                       capture_output=True, text=True, encoding="utf-8").stdout.split("\n")
files = [n for n in names if n.strip()]
tree_items = []
for rel in files:
    # ★ 必须用 git 眼中的内容（已做 CRLF→LF 规范化），不能直接读工作区文件：
    #   否则远程 blob 与本地 commit 的 blob 不一致，下次 fetch 会看到"整文件都被改过"
    data = subprocess.run(["git", "-C", ROOT, "show", f"HEAD:{rel}"], capture_output=True).stdout
    blob = api("POST", "/git/blobs", {"content": base64.b64encode(data).decode(), "encoding": "base64"})
    if "sha" not in blob:
        raise SystemExit(f"blob failed for {rel}: {blob}")
    tree_items.append({"path": rel, "mode": "100644", "type": "blob", "sha": blob["sha"]})
    print(f"  blob {rel}  {len(data)}B  {blob['sha'][:12]}")

# 3) tree / commit
tree = api("POST", "/git/trees", {"base_tree": base_tree, "tree": tree_items})
if "sha" not in tree:
    raise SystemExit(f"tree failed: {tree}")
print("new tree", tree["sha"][:12])

commit = api("POST", "/git/commits", {"message": MSG, "tree": tree["sha"], "parents": [PARENT]})
if "sha" not in commit:
    raise SystemExit(f"commit failed: {commit}")
print("new commit", commit["sha"])

# 4) 更新分支 + 建 tag
upd = api("PATCH", "/git/refs/heads/main", {"sha": commit["sha"], "force": True})
print("ref main ->", upd.get("object", {}).get("sha", upd)[:20] if isinstance(upd, dict) else upd)

tag = api("POST", "/git/refs", {"ref": f"refs/tags/{TAG}", "sha": commit["sha"]})
print("tag", TAG, "->", tag.get("ref", tag))
