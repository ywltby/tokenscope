"""升级测试起点登记表：离线选择起点、登记发布资产、校验远端身份。

`select` 是离线纯函数（只读检出提交的登记表，不访问网络），标准输出只有
JSON；`add` / `withdraw` 写登记表；`check` 读 GitHub 校验已登记条目。

用法：
  python -B scripts/upgrade_baselines.py select --current-version 0.3.3
  python -B scripts/upgrade_baselines.py entries
  python -B scripts/upgrade_baselines.py add --tag v0.3.3
  python -B scripts/upgrade_baselines.py add --tag v0.3.3 --unpublished \
      --commit <sha> --reason "发布流水线失败"
  python -B scripts/upgrade_baselines.py withdraw --version 0.3.3 --reason "崩溃" \
      --date 2026-10-20
  python -B scripts/upgrade_baselines.py check --base-ref origin/main

退出码：0 = 通过；1 = 校验失败（消息打到 stderr）；2 = 用法/参数错误。
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from bump_version import historical_versions
from release_assets import digest, safe_name

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = ROOT / "scripts/fixtures/upgrade_baselines.json"
DEFAULT_EXPECTED = "scripts/fixtures/upgrade-0.1.0/expected.json"
PERSISTENT_KEYS = ("cache_schema", "history_schema", "parser_revision",
                   "identity_revision", "pricing_index", "settings_format")
STATUSES = ("published", "withdrawn", "unpublished")
PROFILES = ("legacy-cache", "history")
SOURCE_STATES = ("present", "deleted")
LEGACY_VERSION = "0.1.0"
MAX_ORIGINS = 6
HASH = re.compile(r"[0-9a-f]{64}")
# 传输层瞬断（代理 TLS EOF、连接重置、超时）的总尝试次数；HTTP 状态错误不重试。
NETWORK_ATTEMPTS = 3
COMMIT = re.compile(r"[0-9a-f]{40}")
VERSION = re.compile(r"0\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
DATE = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}")
REPOSITORY = re.compile(r"[\w.-]+/[\w.-]+")


def version_tuple(version):
    match = VERSION.fullmatch(str(version))
    if match is None:
        raise ValueError(f"版本号非法：{version!r}")
    return tuple(int(part) for part in match.groups())


def parse_registry(text, source="scripts/fixtures/upgrade_baselines.json"):
    """校验登记表结构；文本入口与文件入口共用同一份规则。"""
    payload = json.loads(text)
    if not isinstance(payload, dict) or payload.get("schema_version") != 2:
        raise ValueError(f"登记表 schema_version 必须为 2：{source}")
    repository = payload.get("repository")
    if not isinstance(repository, str) or REPOSITORY.fullmatch(repository) is None:
        raise ValueError("登记表仓库名非法")
    entries = payload.get("entries")
    if not isinstance(entries, list) or not entries:
        raise ValueError("登记表必须包含条目")
    seen = set()
    for entry in entries:
        validate_entry(entry, repository)
        if entry["version"] in seen:
            raise ValueError(f"登记表版本重复：{entry['version']}")
        seen.add(entry["version"])
    return payload


def load_registry(path=REGISTRY):
    return parse_registry(Path(path).read_text(encoding="utf-8"), source=str(path))


def entry_of(registry, version):
    entry = next((e for e in registry["entries"] if e["version"] == version), None)
    if entry is None:
        raise ValueError(f"登记表没有 {version} 条目")
    return entry


def published_entry(registry, version):
    entry = entry_of(registry, version)
    if entry["status"] != "published":
        raise ValueError(f"{version} 登记为 {entry['status']}，不能作为发布或升级来源")
    return entry


def validate_entry(entry, repository):
    if not isinstance(entry, dict):
        raise ValueError("登记条目必须是对象")
    version = entry.get("version")
    version_tuple(version)
    if entry.get("tag") != "v" + str(version):
        raise ValueError(f"登记条目标签与版本不一致：{version!r}")
    if COMMIT.fullmatch(str(entry.get("commit", ""))) is None:
        raise ValueError(f"登记条目提交非法：{version!r}")
    if entry.get("origin_release") not in ("v" + str(version), "pre-" + str(entry["commit"])):
        raise ValueError(f"登记条目来源发布非法：{version!r}")
    if entry.get("profile") not in PROFILES:
        raise ValueError(f"登记条目起点夹具类型非法：{version!r}")
    if entry["profile"] == "legacy-cache" and str(version) != LEGACY_VERSION:
        raise ValueError(f"只有 {LEGACY_VERSION} 是 cache.db 起点：{version!r}")
    status = entry.get("status")
    if status not in STATUSES:
        raise ValueError(f"登记条目状态非法：{version!r}")
    if status == "withdrawn":
        withdrawn = entry.get("withdrawn")
        if (not isinstance(withdrawn, dict) or DATE.fullmatch(str(withdrawn.get("date", ""))) is None
                or not str(withdrawn.get("reason", "")).strip()):
            raise ValueError(f"撤回条目必须写明日期与原因：{version!r}")
    validate_persistent_format(entry.get("persistent_format"), version, status)
    validate_privacy_revision(entry, version)
    validate_expectations(entry, version)
    validate_assets(entry, repository, status)


def validate_privacy_revision(entry, version):
    """设置格式 3 起的版本认识隐私修订号：登记必须携带该基线自己认识的
    修订号；布尔时代（格式 < 3）基线不得登记（编排旧阶段播种同意与
    WB05 闸门观察适用性都取自这个字段）。"""
    value = entry.get("privacy_revision")
    fmt = (entry.get("persistent_format") or {}).get("settings_format")
    if value is None:
        if fmt is not None and int(fmt) >= 3:
            raise ValueError(f"设置格式 3 起的登记版本必须登记 privacy_revision：{version!r}")
        return
    if isinstance(value, bool) or not isinstance(value, int) or value < 1:
        raise ValueError(f"privacy_revision 必须是正整数：{version!r}")
    if fmt is None or int(fmt) < 3:
        raise ValueError(f"不认识修订号的版本不得登记 privacy_revision：{version!r}")


def validate_persistent_format(value, version, status="published"):
    if value is None:
        if status != "unpublished":
            raise ValueError(f"已登记版本必须有持久格式指纹：{version!r}")
        return
    if not isinstance(value, dict) or set(value) != set(PERSISTENT_KEYS):
        raise ValueError(f"持久格式必须是六个固定字段：{version!r}")
    for key, item in value.items():
        if item is None:
            continue
        if not isinstance(item, str) or not item.strip() or item != item.strip():
            raise ValueError(f"持久格式取值非法：{version!r} {key}")


def validate_expectations(entry, version):
    expected_old = entry.get("expected_old")
    if expected_old is not None and not isinstance(expected_old, str):
        raise ValueError(f"expected_old 必须是路径：{version!r}")
    expected_after = entry.get("expected_after")
    if expected_after is None:
        return
    if (not isinstance(expected_after, dict) or set(expected_after) != set(SOURCE_STATES)
            or any(not isinstance(value, str) for value in expected_after.values())):
        raise ValueError(f"expected_after 必须同时给出 present 与 deleted：{version!r}")


def validate_assets(entry, repository, status):
    version, assets = entry["version"], entry.get("assets")
    if status == "unpublished":
        if assets:
            raise ValueError(f"未发布版本不得登记资产：{version!r}")
        return
    if not isinstance(assets, list) or len(assets) < 3:
        raise ValueError(f"已登记版本必须固定原始资产：{version!r}")
    prefix = (f"https://github.com/{repository}/releases/download/"
              f"{entry['origin_release']}/")
    names = []
    for asset in assets:
        if not isinstance(asset, dict):
            raise ValueError(f"资产条目必须是对象：{version!r}")
        name = safe_name(asset.get("name"))
        if HASH.fullmatch(str(asset.get("sha256", ""))) is None or asset.get("url") != prefix + name:
            raise ValueError(f"资产地址或 SHA256 非法：{version!r} {name}")
        names.append(name)
    if len(set(names)) != len(names):
        raise ValueError(f"资产名重复：{version!r}")
    if (sum(name.endswith("-setup.exe") for name in names) != 1
            or "SHA256SUMS.txt" not in names
            or "provenance.sigstore.json" not in names):
        raise ValueError(f"资产集合非法：{version!r}")


def installer_asset(entry):
    """已登记版本必须恰有一个安装包；未发布版本没有资产。"""
    return next((a for a in entry.get("assets") or [] if a["name"].endswith("-setup.exe")), None)


def resolved_entry(entry, root=ROOT):
    """给编排用的条目视图：期望清单路径与安装包名都按缺省规则解析成绝对路径。"""
    state_paths = {state: str(root / ((entry.get("expected_after") or {}).get(state)
                                     or DEFAULT_EXPECTED)) for state in SOURCE_STATES}
    installer = installer_asset(entry)
    return {"version": entry["version"], "tag": entry["tag"], "commit": entry["commit"],
            "origin_release": entry["origin_release"], "profile": entry["profile"],
            "status": entry["status"],
            "persistent_format": entry.get("persistent_format"),
            "privacy_revision": entry.get("privacy_revision"),
            "expected_old": str(root / (entry.get("expected_old") or DEFAULT_EXPECTED)),
            "expected_after": state_paths,
            "installer_name": None if installer is None else installer["name"],
            "installer_sha256": None if installer is None else installer["sha256"]}


def select_origins(registry, current_version):
    """起点集合：0.1.0 + 上一版 + 每种持久格式的代表，去重后按版本升序。"""
    current = version_tuple(current_version)
    published = [e for e in registry["entries"] if e["status"] == "published"]
    baseline = next((e for e in published if e["version"] == LEGACY_VERSION), None)
    if baseline is None or version_tuple(baseline["version"]) >= current:
        raise ValueError(
            f"登记表缺少低于 {current_version} 的 {LEGACY_VERSION} 起点："
            "撤回或缺失都必须先重新评审起点集合")
    eligible = [e for e in published if version_tuple(e["version"]) < current]
    previous = max(eligible, key=lambda e: version_tuple(e["version"]))
    selected = {baseline["version"]: baseline, previous["version"]: previous}
    groups = {}
    for entry in eligible:
        key = representation_key(entry)
        known = groups.get(key)
        if known is None or version_tuple(known["version"]) < version_tuple(entry["version"]):
            groups[key] = entry
    for entry in groups.values():
        selected[entry["version"]] = entry
    ordered = sorted(selected.values(), key=lambda e: version_tuple(e["version"]))
    if len(ordered) > MAX_ORIGINS:
        names = "、".join(e["version"] for e in ordered)
        raise ValueError(f"起点数 {len(ordered)} 超过 {MAX_ORIGINS}（{names}）："
                         "回到计划评审，不静默扩张")
    return ordered


def representation_key(entry):
    return json.dumps({key: entry["persistent_format"][key] for key in PERSISTENT_KEYS},
                      sort_keys=True)


def selection_payload(registry, current_version):
    origins = [entry["version"] for entry in select_origins(registry, current_version)]
    return {"schema_version": 2, "current_version": str(current_version),
            "origins": origins, "chain": list(origins),
            "source_states": list(SOURCE_STATES),
            "include": [{"origin": version, "source_state": state}
                        for version in origins for state in SOURCE_STATES]}


def with_network_retry(action, sleep=time.sleep):
    """执行一次网络读取；只对传输层瞬断有限重试，用尽后原样抛出最后一个错误。

    HTTP 状态错误（404、403 限流等）是服务端的明确答复，立即抛出不重试。
    """
    for attempt in range(1, NETWORK_ATTEMPTS + 1):
        try:
            return action()
        except HTTPError:
            raise
        except OSError:
            if attempt == NETWORK_ATTEMPTS:
                raise
            sleep(attempt)


def local_gh_token():
    """本机未设令牌时借用已登录 gh 的令牌，避免匿名 API 每小时 60 次的限流。

    令牌只随 api.github.com 请求发送；gh 不存在、未登录或超时都返回 None。
    """
    try:
        result = subprocess.run(["gh", "auth", "token"], capture_output=True, text=True,
                                timeout=10, check=False)
    except (OSError, subprocess.TimeoutExpired):
        return None
    token = result.stdout.strip()
    return token if result.returncode == 0 and token else None


class Api:
    """只读 GitHub REST 客户端；读取失败一律抛错，不静默跳过。"""

    def __init__(self, repository, token=None):
        if REPOSITORY.fullmatch(str(repository)) is None:
            raise ValueError("仓库名称非法")
        self.repository = repository
        self.token = (token or os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
                      or local_gh_token())

    def get(self, path):
        request = Request("https://api.github.com" + path, headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "tokenscope-upgrade-baselines"})
        if self.token:
            request.add_header("Authorization", "Bearer " + self.token)
        def fetch():
            with urlopen(request, timeout=60) as response:
                return json.loads(response.read().decode("utf-8"))

        try:
            return with_network_retry(fetch)
        except HTTPError as exc:
            if exc.code == 404:
                return None
            raise RuntimeError(f"GitHub 读取失败：{path} HTTP {exc.code}") from exc

    def release(self, tag):
        return self.get(f"/repos/{self.repository}/releases/tags/{tag}")

    def tag_commit(self, tag):
        payload = self.get(f"/repos/{self.repository}/commits/{tag}")
        return None if payload is None else payload.get("sha")


def download(url, sha256, directory, name):
    """下载资产到目录并按给定哈希校验；返回（路径，实际 SHA256）。"""
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / safe_name(name)
    temporary = path.with_name(path.name + ".download")
    def fetch():
        with urlopen(url, timeout=180) as response, temporary.open("wb") as fh:
            while chunk := response.read(1024 * 1024):
                fh.write(chunk)

    try:
        with_network_retry(fetch)
        actual = digest(temporary)
        if sha256 is not None and actual != sha256:
            raise ValueError(f"资产 SHA256 与登记值不一致，拒绝使用：{name}")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)
    return path, actual


def check_assets(entry, api, directory, warnings):
    """核对一个 published 条目的远端身份，并把 build-metadata.json 载荷读回来。"""
    version, tag = entry["version"], entry["tag"]
    if api.tag_commit(tag) != entry["commit"]:
        raise ValueError(f"{version} 的标签不再解引用到登记提交：请核对或标为 withdrawn")
    release = api.release(tag)
    if release is None or release.get("draft"):
        raise ValueError(f"{version} 的发布已删除或仍是草稿：请核对或标为 withdrawn")
    actual = {}
    for asset in release.get("assets", []):
        name = safe_name(asset.get("name"))
        if name in actual:
            raise ValueError(f"{version} 的远端资产名重复：{name}")
        actual[name] = asset
    expected = {asset["name"]: asset for asset in entry["assets"]}
    if set(actual) != set(expected):
        raise ValueError(f"{version} 的远端资产集合与登记不一致：请核对或标为 withdrawn")
    for name, asset in expected.items():
        remote = (actual[name].get("digest") or "").removeprefix("sha256:")
        if HASH.fullmatch(remote) is not None:
            if remote != asset["sha256"]:
                raise ValueError(f"{version} 的远端资产已被替换：{name}")
            continue
        warnings.append(f"{version} 的 {name} 缺少 API digest，已下载复核")
        download(actual[name]["browser_download_url"], asset["sha256"], directory, name)
    metadata_asset = expected.get("build-metadata.json")
    if metadata_asset is None:
        return None
    path, _ = download(actual["build-metadata.json"]["browser_download_url"],
                       metadata_asset["sha256"], directory, "build-metadata.json")
    metadata = json.loads(path.read_text(encoding="utf-8"))
    validate_metadata_format(metadata, entry)
    return metadata


def validate_metadata_format(metadata, entry):
    """本 PR 之后发布的版本自带持久格式指纹，必须与登记值一致。"""
    if not isinstance(metadata, dict):
        raise ValueError(f"{entry['version']} 的构建元数据不是对象")
    if metadata.get("schema_version") != 2:
        return
    value = metadata.get("persistent_format")
    if value != entry["persistent_format"]:
        raise ValueError(f"{entry['version']} 的登记持久格式与 build-metadata.json 不一致："
                         f"登记 {entry['persistent_format']}，元数据 {value}")


def base_version(root, base_ref):
    """基础版本按该 ref 自身的仓库布局读取（迁移前后 manifest 位置不同）。"""
    _, version = historical_versions(root, base_ref)
    return version


def check_registry(base_ref, path=REGISTRY, api=None):
    registry = load_registry(path)
    resolved = base_version(ROOT, base_ref)
    base_entry = next((e for e in registry["entries"] if e["version"] == resolved), None)
    if base_entry is None:
        raise ValueError(f"基础版本 {resolved}（{base_ref}）没有登记：先运行 "
                         f"python -B scripts/upgrade_baselines.py add --tag v{resolved}")
    api = api or Api(registry["repository"])
    warnings, checked = [], []
    if base_entry["status"] != "published":
        # unpublished（发布流水线失败）与 withdrawn 都允许作为基础版本继续合入，
        # 但必须在结果里显式提示，避免"上一版没公开"被当成已验收。
        warnings.append(f"基础版本 {resolved} 登记为 {base_entry['status']}："
                        "该版本不作为升级起点，其发布或撤回状态请按计划第 4.4 节处理")
    with tempfile.TemporaryDirectory(prefix="tokenscope-baselines-") as temporary:
        for entry in registry["entries"]:
            if entry["status"] == "withdrawn":
                if api.release(entry["tag"]) is not None:
                    warnings.append(f"{entry['version']} 已撤回，但发布页仍存在：请删除发布页")
                continue
            if entry["status"] == "unpublished":
                continue
            check_assets(entry, api, Path(temporary), warnings)
            checked.append(entry["version"])
    return {"ok": True, "base_version": resolved, "checked": checked, "warnings": warnings}


def register(entry, path=REGISTRY):
    registry = load_registry(path)
    if any(item["version"] == entry["version"] for item in registry["entries"]):
        raise ValueError(f"版本已登记：{entry['version']}")
    registry["entries"].append(entry)
    registry["entries"].sort(key=lambda item: version_tuple(item["version"]))
    # 写盘前整表校验：新条目缺/坏元数据（如格式 3 缺 privacy_revision）在
    # 这里失败，登记表保持原样，不能把「下一次 load 拒读」的坏表写出去。
    parse_registry(json.dumps(registry, ensure_ascii=False), source=str(path))
    write_registry(registry, path)
    return entry


def write_registry(registry, path=REGISTRY):
    text = json.dumps(registry, ensure_ascii=False, indent=2) + "\n"
    Path(path).write_text(text, encoding="utf-8", newline="\n")


def add_entry(tag, api=None, path=REGISTRY, directory=None, status="published",
              commit="", reason=""):
    """登记一个版本的发布资产；只有全部资产与元数据核对通过才写入登记表。"""
    version = tag[1:] if str(tag).startswith("v") else str(tag)
    version_tuple(version)
    registry = load_registry(path)
    if any(item["tag"] == "v" + version for item in registry["entries"]):
        raise ValueError(f"版本已登记：{version}")
    api = api or Api(registry["repository"])
    if status == "unpublished":
        if COMMIT.fullmatch(str(commit)) is None or not str(reason).strip():
            raise ValueError("登记未发布版本必须给出 --commit 与 --reason")
        return register({"version": version, "tag": "v" + version, "commit": commit,
                         "origin_release": "v" + version, "profile": profile_for(version),
                         "status": "unpublished", "persistent_format": None, "assets": []}, path)
    resolved = api.tag_commit("v" + version)
    if resolved is None:
        raise ValueError(f"等待 v{version} 的标签与发布完成后再登记")
    release = api.release("v" + version)
    if release is None or release.get("draft") or release.get("tag_name") != "v" + version:
        raise ValueError(f"等待 v{version} 的发布完成后再登记")
    with tempfile.TemporaryDirectory(prefix="tokenscope-baselines-add-") as temporary:
        work = Path(directory or temporary)
        assets = []
        for raw in release.get("assets", []):
            name = safe_name(raw.get("name"))
            url = raw.get("browser_download_url")
            if not isinstance(url, str) or not url.startswith("https://github.com/"):
                raise ValueError(f"资产缺少下载地址：{name}")
            remote = (raw.get("digest") or "").removeprefix("sha256:")
            remote = remote if HASH.fullmatch(remote) else None
            _, actual = download(url, remote, work, name)
            assets.append({"name": name, "url": url, "sha256": actual})
        metadata = read_added_metadata(work, assets, version)
    privacy_revision = None
    if int(metadata["settings_format"]) >= 3:
        # 设置格式 3 起的版本认识隐私修订号：登记必须取该发布标签自身源码
        # 里的 PRIVACY_POLICY_REVISION，不能盲用登记时当前源码的修订号。
        privacy_revision = read_tag_privacy_revision(resolved)
        if privacy_revision is None:
            raise ValueError(f"v{version} 的标签源码没有 PRIVACY_POLICY_REVISION，"
                             "拒绝登记设置格式 3 版本")
    entry = {"version": version, "tag": "v" + version, "commit": resolved,
             "origin_release": "v" + version, "profile": profile_for(version),
             "status": "published", "persistent_format": metadata, "assets": assets}
    if privacy_revision is not None:
        entry["privacy_revision"] = privacy_revision
        # 0.6.1 起的版本在发布源码里要求用户许可协议同意：登记表必须如实
        # 记录目标版本的协议能力，后续以它为历史目标时才能装配正确门禁。
        if read_tag_agreement_required(resolved):
            entry["agreement_revision"] = privacy_revision
    return register(entry, path)


def strip_rust_comments_and_strings(text):
    """去掉 Rust 注释与字符串字面量（保留换行），供常量声明正则使用——
    避免块注释/原始字符串里的假常量被当成真实声明（十八轮复核 P3）。"""
    out = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < n else ""
        if ch == "/" and nxt == "/":
            while i < n and text[i] != "\n":
                i += 1
        elif ch == "/" and nxt == "*":
            depth = 1
            i += 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    if text[i] == "\n":
                        out.append("\n")
                    i += 1
        elif ch == "r" and nxt in ('"', "#"):
            # 原始字符串 r"..." / r#"..."# / r##"..."##
            j = i + 1
            hashes = 0
            while j < n and text[j] == "#":
                hashes += 1
                j += 1
            if j < n and text[j] == '"':
                closing = '"' + "#" * hashes
                j += 1
                while j < n and not text.startswith(closing, j):
                    if text[j] == "\n":
                        out.append("\n")
                    j += 1
                i = j + len(closing)
                continue
            out.append(ch)
            i += 1
        elif ch == "'":
            # 字符字面量整体跳过；生命周期（&static str / 单引号+标识符）
            # 不是字面量——二十轮复核 P3：按字面量处理会吞掉后续常量。
            if nxt == "\\" and i + 3 < n and text[i + 3] == "'":
                i += 4  # 转义字符字面量
                continue
            if i + 2 < n and text[i + 2] == "'" and nxt not in ("\n", "'"):
                i += 3  # 单字符字面量
                continue
            i += 1  # 生命周期：只跳过引号本身，标识符按普通文本保留
        elif ch == '"':
            i += 1
            while i < n:
                if text[i] == "\\":
                    i += 2
                    continue
                if text[i] == '"':
                    i += 1
                    break
                if text[i] == "\n":
                    out.append("\n")
                i += 1
        else:
            out.append(ch)
            i += 1
    return "".join(out)


PRIVACY_REVISION_SOURCES = ("crates/tokenscope-core/src/settings.rs", "src/settings.rs")


def extract_privacy_revision(source):
    """从 Rust 源码提取共同同意修订号，忽略注释和字符串中的假声明。"""
    match = re.search(
        r"(?m)^(?:pub\s+)?const\s+PRIVACY_POLICY_REVISION\s*:\s*u8\s*=\s*(\d+)\s*;",
        strip_rust_comments_and_strings(source),
    )
    return int(match.group(1)) if match else None


def supports_user_agreement(source):
    """目标源码是否认识用户许可协议同意字段（0.6.1 起）。注释与字符串里的
    假字段同样忽略，避免把历史版本误判为协议感知版本。"""
    stripped = strip_rust_comments_and_strings(source)
    return re.search(
        r"(?m)^(?:pub\s+)?user_agreement_(?:accepted|revision)\s*:",
        stripped,
    ) is not None


def read_tag_privacy_revision(commit, root=ROOT):
    """从发布标签解引用到的源码读隐私修订号；两处历史路径都找不到常量
    时返回 None（由调用方决定是否拒绝登记）。"""
    for relative in PRIVACY_REVISION_SOURCES:
        result = subprocess.run(["git", "-C", str(root), "show", f"{commit}:{relative}"],
                                capture_output=True, encoding="utf-8", check=False)
        if result.returncode:
            continue
        revision = extract_privacy_revision(result.stdout)
        if revision is not None:
            return revision
    return None


def read_tag_agreement_required(commit, root=ROOT):
    """发布标签源码是否要求用户许可协议同意（与隐私政策同一次同意）。"""
    for relative in PRIVACY_REVISION_SOURCES:
        result = subprocess.run(["git", "-C", str(root), "show", f"{commit}:{relative}"],
                                capture_output=True, encoding="utf-8", check=False)
        if result.returncode:
            continue
        if extract_privacy_revision(result.stdout) is not None:
            return supports_user_agreement(result.stdout)
    return False


def profile_for(version):
    return "legacy-cache" if version == LEGACY_VERSION else "history"


def read_added_metadata(directory, assets, version):
    if not any(a["name"] == "build-metadata.json" for a in assets):
        raise ValueError(f"{version} 的发布缺少 build-metadata.json，无法登记持久格式")
    payload = json.loads((Path(directory) / "build-metadata.json").read_text(encoding="utf-8"))
    value = payload.get("persistent_format") if isinstance(payload, dict) else None
    if (not isinstance(payload, dict) or payload.get("schema_version") != 2
            or not isinstance(value, dict)):
        raise ValueError(f"{version} 的构建元数据缺少 persistent_format："
                         "按标签源码手工登记后再合入")
    validate_persistent_format(value, version)
    return {key: value[key] for key in PERSISTENT_KEYS}


def withdraw_entry(version, reason, date, path=REGISTRY):
    registry = load_registry(path)
    entry = next((e for e in registry["entries"] if e["version"] == version), None)
    if entry is None:
        raise ValueError(f"版本没有登记：{version}")
    if DATE.fullmatch(str(date)) is None or not str(reason).strip():
        raise ValueError("撤回必须给出 --date 与 --reason")
    entry["status"] = "withdrawn"
    entry["withdrawn"] = {"date": date, "reason": reason}
    write_registry(registry, path)
    return entry


def entries_payload(registry):
    return {"schema_version": 2, "repository": registry["repository"],
            "entries": [resolved_entry(entry) for entry in registry["entries"]]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    command = sub.add_parser("select")
    command.add_argument("--current-version", required=True)
    command = sub.add_parser("entries")
    command = sub.add_parser("privacy-revision")
    command.add_argument("--source", type=Path,
                         default=ROOT / "crates/tokenscope-core/src/settings.rs")
    command = sub.add_parser("add")
    command.add_argument("--tag", required=True)
    command.add_argument("--unpublished", action="store_true")
    command.add_argument("--commit", default="")
    command.add_argument("--reason", default="")
    command = sub.add_parser("withdraw")
    command.add_argument("--version", required=True)
    command.add_argument("--reason", required=True)
    command.add_argument("--date", required=True)
    command = sub.add_parser("check")
    command.add_argument("--base-ref", default="origin/main")
    command.add_argument("--registry", type=Path, default=REGISTRY)
    args = parser.parse_args()
    try:
        if args.mode == "select":
            result = selection_payload(load_registry(), args.current_version)
        elif args.mode == "entries":
            result = entries_payload(load_registry())
        elif args.mode == "privacy-revision":
            revision = extract_privacy_revision(args.source.read_text(encoding="utf-8"))
            if revision is None:
                raise ValueError(f"{args.source} 缺少 PRIVACY_POLICY_REVISION 常量")
            result = {"privacy_revision": revision}
        elif args.mode == "add":
            entry = add_entry(args.tag,
                              status="unpublished" if args.unpublished else "published",
                              commit=args.commit, reason=args.reason)
            result = {"added": entry["version"], "status": entry["status"]}
        elif args.mode == "withdraw":
            entry = withdraw_entry(args.version, args.reason, args.date)
            result = {"withdrawn": entry["version"], "status": entry["status"]}
        else:
            result = check_registry(args.base_ref, path=args.registry)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0
    except (ValueError, OSError, KeyError, RuntimeError, subprocess.CalledProcessError) as exc:
        print(f"升级起点登记表校验失败：{exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    sys.exit(main())
