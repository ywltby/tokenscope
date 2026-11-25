#!/usr/bin/env python3
"""从 models.dev 公开目录重建嵌入的展示名数据。

models.json 提供规范模型 ID 与模型级 name（无价格）；api.json 的
canonical_model_id 是发布目录显式给出的身份关系，用于补充规范清单无法按
末段命中的别名（例如 doubao-seed-2-0-pro-260215 -> bytedance-seed/seed-2.0-pro），
以及末段相同但发布方不同的显式转发（relay/demo-v2 -> publisher/demo-v2）。
末段相同不构成身份证据，解析侧只把该关系用于没有自身规范名的完整 ID。
有提交证据的历史身份由 scripts/fixtures/model_display_catalog_history.json 补回；
当前上游名称与改指向优先，不自动合并整份旧目录。

用法：
    python -B scripts/update_model_display_catalog.py
    python -B scripts/update_model_display_catalog.py --models <models.json> --api <api.json>
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "crates" / "tokenscope-core" / "data" / "model_display_catalog.json"
HISTORY = ROOT / "scripts" / "fixtures" / "model_display_catalog_history.json"
MODELS_URL = "https://models.dev/models.json"
API_URL = "https://models.dev/api.json"


def fetch(url: str) -> dict:
    proxy = os.environ.get("HTTPS_PROXY") or os.environ.get("https_proxy")
    opener = (
        urllib.request.build_opener(urllib.request.ProxyHandler({"https": proxy}))
        if proxy
        else urllib.request.build_opener()
    )
    request = urllib.request.Request(
        url, headers={"User-Agent": "TokenScope-catalog-maintainer"}
    )
    with opener.open(request, timeout=300) as response:
        return json.load(response)


def build(models: dict, api: dict, captured_at: str, history: dict | None = None) -> dict:
    canonical = {}
    aliases = {}
    if history is not None:
        for entry in history.get("models", []):
            canonical[entry["id"]] = entry["name"]
        for entry in history.get("aliases", []):
            target = entry["canonical_id"]
            if target == entry["id"]:
                raise ValueError(f"历史别名不得自指: {entry['id']}")
            aliases[entry["id"]] = target
    canonical.update(
        {
            entry["id"]: entry["name"]
            for entry in models.values()
            if isinstance(entry, dict) and entry.get("id") and entry.get("name")
        }
    )
    for provider, provider_body in api.items():
        for model_id, body in provider_body.get("models", {}).items():
            target = body.get("canonical_model_id") if isinstance(body, dict) else None
            own_id = f"{provider}/{model_id}"
            if not isinstance(target, str) or target not in canonical or target == own_id:
                continue
            aliases[own_id] = target
    for alias, target in aliases.items():
        if target not in canonical:
            raise ValueError(f"历史别名目标不在最终规范清单: {alias} -> {target}")
    preserved_models = sum(
        entry["id"] in canonical and canonical[entry["id"]] == entry["name"]
        for entry in (history or {}).get("models", [])
    )
    preserved_aliases = sum(
        aliases.get(entry["id"]) == entry["canonical_id"]
        for entry in (history or {}).get("aliases", [])
    )
    return {
        "source": {
            "models": MODELS_URL,
            "aliases": API_URL,
            "captured_at": captured_at,
            "note": "规范名来自 models.json；别名只取 api.json 中显式 canonical_model_id、目标在规范清单且不等于自身的条目。",
            "history": {
                "fixture": "scripts/fixtures/model_display_catalog_history.json",
                "models": preserved_models,
                "aliases": preserved_aliases,
                "note": "仅补回有提交证据的历史身份；当前上游名称与改指向优先，不自动合并旧目录。",
            },
        },
        "models": [
            {"id": model_id, "name": name}
            for model_id, name in sorted(canonical.items())
        ],
        "aliases": [
            {"id": alias, "canonical_id": target}
            for alias, target in sorted(aliases.items())
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--models", help="本地 models.json（离线重建）")
    parser.add_argument("--api", help="本地 api.json（离线重建）")
    parser.add_argument("--captured-at")
    args = parser.parse_args()
    if bool(args.models) != bool(args.api):
        parser.error("--models 与 --api 必须同时提供")

    models = (
        json.loads(pathlib.Path(args.models).read_text(encoding="utf-8"))
        if args.models
        else fetch(MODELS_URL)
    )
    api = (
        json.loads(pathlib.Path(args.api).read_text(encoding="utf-8"))
        if args.api
        else fetch(API_URL)
    )
    captured_at = args.captured_at or dt.datetime.now(dt.timezone.utc).date().isoformat()
    history = json.loads(HISTORY.read_text(encoding="utf-8"))
    catalog = build(models, api, captured_at, history)
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    with OUTPUT.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n")
    print(f"{OUTPUT}: {len(catalog['models'])} models, {len(catalog['aliases'])} aliases")
    return 0


if __name__ == "__main__":
    sys.exit(main())
