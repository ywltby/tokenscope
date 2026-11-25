#!/usr/bin/env python3
"""从 models.dev 公开目录重建嵌入的展示名数据。

models.json 提供规范模型 ID 与模型级 name（无价格）；api.json 的
canonical_model_id 是发布目录显式给出的身份关系，用于补充规范清单无法按
末段命中的别名（例如 doubao-seed-2-0-pro-260215 -> bytedance-seed/seed-2.0-pro），
以及末段相同但发布方不同的显式转发（relay/demo-v2 -> publisher/demo-v2）。
末段相同不构成身份证据，解析侧只把该关系用于没有自身规范名的完整 ID。
有提交证据的历史身份由 scripts/fixtures/model_display_catalog_history.json 补回：
仅对当前目录里已经消失的条目生效，且排除仅留档的弱证据（upstream_assumed）；
当前条目存在时一律以现行目录为准，不自动合并整份旧目录。

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
_SEPARATORS = frozenset("-._")
TRUSTED_EVIDENCE_STRENGTHS = frozenset({None, "verified"})
ARCHIVED_EVIDENCE_STRENGTHS = frozenset({"upstream_assumed"})


def _ascii_lower(ch: str) -> str:
    return ch.lower() if "A" <= ch <= "Z" else ch


def identity_key(raw: str) -> str | None:
    """与 Rust `ModelIdentity::identity_key` 一致的完整 ID 等价键。

    先 trim，保留 `/` 与 `:` 结构语义，删除 ASCII `-` `.` `_` 并仅折叠 ASCII 大小写；
    基名（末段 `:` 之前）压缩为空时返回 None。
    """
    trimmed = raw.strip()
    leaf = trimmed.rsplit("/", 1)[-1]
    base = leaf.split(":", 1)[0]
    if not any(ch not in _SEPARATORS for ch in base):
        return None
    out: list[str] = []
    for ch in trimmed:
        if ch in "/:":
            out.append(ch)
        elif ch in _SEPARATORS:
            continue
        else:
            out.append(_ascii_lower(ch))
    return "".join(out)


def _is_trusted_history_entry(entry: dict) -> bool:
    strength = entry.get("evidence_strength")
    if strength in TRUSTED_EVIDENCE_STRENGTHS:
        return True
    if strength in ARCHIVED_EVIDENCE_STRENGTHS:
        return False
    raise ValueError(f"未识别的历史证据等级: {entry.get('id')} ({strength!r})")


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
    canonical: dict[str, str] = {}
    canonical_keys: set[str] = set()
    for entry in models.values():
        if isinstance(entry, dict) and entry.get("id") and entry.get("name"):
            model_id = entry["id"]
            canonical[model_id] = entry["name"]
            key = identity_key(model_id)
            if key is not None:
                canonical_keys.add(key)

    preserved_models = 0
    if history is not None:
        history_model_keys: set[str] = set()
        for entry in history.get("models", []):
            if not _is_trusted_history_entry(entry):
                continue
            model_id = entry["id"]
            key = identity_key(model_id)
            if model_id in canonical or (key is not None and key in canonical_keys):
                continue
            if key is not None and key in history_model_keys:
                raise ValueError(f"历史规范模型重复: {model_id}")
            canonical[model_id] = entry["name"]
            if key is not None:
                history_model_keys.add(key)
            preserved_models += 1

    # 分别判定「当前条目是否存在」与「当前映射是否可用」：
    # 存在性按与 Rust 一致的完整 ID 等价键检查，防止仅大小写/分隔符变化被误判退休；
    # 现行条目存在但关系缺失、自指或指向未收录目标时，不回退历史映射。
    current_api_ids: set[str] = set()
    current_api_keys: set[str] = set()
    aliases: dict[str, str] = {}
    for provider, provider_body in api.items():
        if not isinstance(provider_body, dict):
            continue
        provider_models = provider_body.get("models", {})
        if not isinstance(provider_models, dict):
            continue
        for model_id, body in provider_models.items():
            own_id = f"{provider}/{model_id}"
            current_api_ids.add(own_id)
            own_key = identity_key(own_id)
            if own_key is not None:
                current_api_keys.add(own_key)
            target = body.get("canonical_model_id") if isinstance(body, dict) else None
            if not isinstance(target, str) or target not in canonical:
                continue
            target_key = identity_key(target)
            if target == own_id or (own_key is not None and target_key == own_key):
                continue
            aliases[own_id] = target

    preserved_aliases = 0
    if history is not None:
        history_alias_keys: set[str] = set()
        for entry in history.get("aliases", []):
            if not _is_trusted_history_entry(entry):
                continue
            alias_id = entry["id"]
            target = entry["canonical_id"]
            alias_key = identity_key(alias_id)
            if alias_id in current_api_ids or (
                alias_key is not None and alias_key in current_api_keys
            ):
                continue
            target_key = identity_key(target)
            if target == alias_id or (alias_key is not None and target_key == alias_key):
                raise ValueError(f"历史别名不得自指: {alias_id}")
            if target not in canonical:
                raise ValueError(f"历史别名目标不在最终规范清单: {alias_id} -> {target}")
            if alias_id in aliases or (
                alias_key is not None and alias_key in history_alias_keys
            ):
                raise ValueError(f"历史别名重复: {alias_id}")
            aliases[alias_id] = target
            if alias_key is not None:
                history_alias_keys.add(alias_key)
            preserved_aliases += 1

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
                "note": "仅补回当前目录已消失且具备可信提交证据的历史身份；弱证据（upstream_assumed）仅留档不用于取名，当前条目存在时以现行目录为准，不自动合并旧目录。",
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
