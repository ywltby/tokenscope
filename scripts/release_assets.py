"""校验构建资产；历史发布复用原始字节，新发布只在草稿中上传且不覆盖版本。"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from urllib.request import urlopen

from bump_version import declaration, historical_versions, resolve, uniform
from release_metadata import read_versions

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts/fixtures/upgrade_baseline.json"
HASH = re.compile(r"[0-9a-f]{64}")


def digest(path):
    with Path(path).open("rb") as fh:
        return hashlib.file_digest(fh, "sha256").hexdigest()


def safe_name(name):
    if (not isinstance(name, str) or not name or name in (".", "..")
            or any(c in name for c in "/\\:\r\n") or name != name.strip()):
        raise ValueError(f"资产文件名非法：{name!r}")
    return name


def load_baseline(path=BASELINE):
    value = json.loads(Path(path).read_text(encoding="utf-8"))
    if (not isinstance(value, dict) or value.get("schema_version") != 1
            or not re.fullmatch(r"[0-9a-f]{40}", value.get("commit", ""))
            or value.get("version") != "0.1.0" or value.get("tag") != "v0.1.0"
            or value.get("cache_schema") != "6"
            or value.get("origin_release") != "pre-" + value["commit"]
            or not re.fullmatch(r"[\w.-]+/[\w.-]+", value.get("repository", ""))):
        raise ValueError("历史基线身份非法")
    prefix = (f"https://github.com/{value['repository']}/releases/download/"
              f"{value['origin_release']}/")
    assets = value.get("assets")
    if not isinstance(assets, list) or len(assets) != 3:
        raise ValueError("历史基线必须固定三份原始资产")
    names = []
    for asset in assets:
        name = safe_name(asset.get("name"))
        if not HASH.fullmatch(asset.get("sha256", "")) or asset.get("url") != prefix + name:
            raise ValueError("历史资产地址或 SHA256 非法")
        names.append(name)
    if (len(set(names)) != 3 or "SHA256SUMS.txt" not in names
            or "provenance.sigstore.json" not in names
            or sum(n.endswith("-setup.exe") for n in names) != 1):
        raise ValueError("历史资产集合非法")
    return value


def download_baseline(directory, baseline=None):
    baseline = load_baseline() if baseline is None else baseline
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    paths = []
    for asset in baseline["assets"]:
        path = directory / safe_name(asset["name"])
        if not path.is_file() or digest(path) != asset["sha256"]:
            temporary = path.with_name(path.name + ".download")
            try:
                with urlopen(asset["url"], timeout=60) as response, temporary.open("wb") as fh:
                    while chunk := response.read(1024 * 1024):
                        fh.write(chunk)
                if digest(temporary) != asset["sha256"]:
                    raise ValueError(f"旧资产 SHA256 不一致，拒绝使用：{path.name}")
                os.replace(temporary, path)
            finally:
                temporary.unlink(missing_ok=True)
        paths.append(path)
    return paths


def write_checksums(directory, paths):
    paths = list(paths)
    if len({safe_name(p.name) for p in paths}) != len(paths):
        raise ValueError("重复的校验和文件名")
    text = "".join(f"{digest(path)}  {path.name}\n" for path in paths)
    with (Path(directory) / "SHA256SUMS.txt").open("w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def verify_current(directory, commit, version):
    directory = Path(directory)
    installers = list(directory.glob("*-setup.exe"))
    if len(installers) != 1:
        raise ValueError("必须有且只有一个当前安装包")
    expected = {installers[0].name, "build-metadata.json"}
    hashes = {}
    for line in (directory / "SHA256SUMS.txt").read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
        if match is None:
            raise ValueError("SHA256SUMS 格式非法")
        name = safe_name(match[2])
        if name in hashes:
            raise ValueError("SHA256SUMS 文件名重复")
        hashes[name] = match[1]
    if set(hashes) != expected:
        raise ValueError("SHA256SUMS 必须覆盖安装包和构建元数据，不能有额外条目")
    for name, expected_hash in hashes.items():
        if digest(directory / name) != expected_hash:
            raise ValueError(f"资产 SHA256 不一致：{name}")
    metadata = json.loads((directory / "build-metadata.json").read_text(encoding="utf-8"))
    if (metadata.get("schema_version") != 1 or metadata.get("commit") != commit
            or not re.fullmatch(r"[0-9a-f]{40}", commit)
            or metadata.get("version") != version
            or not HASH.fullmatch(metadata.get("executable_sha256", ""))):
        raise ValueError("构建元数据与被测来源、版本或 exe 哈希不一致")
    uniform({"build": version})
    return metadata


def build_metadata(executable, commit, output, root=ROOT):
    if resolve(root, "HEAD") != commit:
        raise ValueError("构建来源 SHA 与实际 checkout 不一致")
    payload = {"schema_version": 1, "commit": commit,
               "version": uniform(read_versions(Path(root))),
               "executable_sha256": digest(executable)}
    Path(output).write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
                            encoding="utf-8")
    return payload


def extract_payload(installer, directory, name="tokenscope.exe", command=None):
    """只读 NSIS 容器，提取唯一根目录主程序；不执行安装器或载荷。"""
    safe_name(name)
    command = command or shutil.which("7z") or shutil.which("7z.exe")
    if command is None:
        bundled = Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "7-Zip/7z.exe"
        if not bundled.is_file():
            raise RuntimeError("缺少 7-Zip，不能核对 NSIS 实际主程序载荷")
        command = str(bundled)
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    if any(directory.iterdir()):
        raise ValueError("主程序提取目录必须为空")

    def run(*args):
        result = subprocess.run([command, *args], capture_output=True,
                                encoding="utf-8-sig", check=False)
        if result.returncode:
            raise RuntimeError(f"NSIS 载荷读取失败：{result.stderr.strip()}")
        return result.stdout

    listing = run("l", "-slt", "-ba", "-sccUTF-8", "-r-", str(installer), name)
    paths = [line[7:] for line in listing.splitlines() if line.startswith("Path = ")]
    if paths != [name]:
        raise ValueError("NSIS 必须包含唯一根目录主程序，拒绝缺失、重复或嵌套路径")
    run("x", "-y", "-sccUTF-8", "-r-", "-o" + str(directory), str(installer), name)
    payload = directory / name
    if (payload.is_symlink() or not payload.is_file() or not payload.stat().st_size
            or not payload.resolve().is_relative_to(directory.resolve())):
        raise ValueError("NSIS 主程序没有被完整提取到临时目录")
    return payload


def collect(bundle, executable, directory, commit, version):
    uniform({"version": version})
    product = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))["productName"]
    installer = Path(bundle) / safe_name(f"{product}_{version}_x64-setup.exe")
    if not installer.is_file():
        raise ValueError(f"当前版本安装包不存在：{installer.name}")
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    if any(directory.iterdir()):
        raise ValueError("当前产物目录必须为空，拒绝混入旧产物")
    target = directory / f"TokenScope_v{version}_windows-x64-setup.exe"
    shutil.copyfile(installer, target)
    metadata = directory / "build-metadata.json"
    # Tauri 打包前写入包类型，打包后还原编译目录的 exe；哈希取安装包的实际字节。
    with tempfile.TemporaryDirectory(prefix="tokenscope-nsis-payload-") as temporary:
        payload = extract_payload(target, temporary, name=Path(executable).name)
        data = build_metadata(payload, commit, metadata)
    data["unbundled_executable_sha256"] = digest(executable)
    metadata.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    write_checksums(directory, [target, metadata])
    return verify_current(directory, commit, version)


def compare_assets(release, expected, complete):
    actual = {}
    for asset in release.get("assets", []):
        name = safe_name(asset.get("name"))
        if name in actual:
            raise ValueError("远端资产名重复")
        actual[name] = asset.get("digest")
    if any(name not in expected or value != expected[name] for name, value in actual.items()):
        raise ValueError("已有远端资产与本次来源不一致，拒绝覆盖")
    if complete and actual != expected:
        raise ValueError("远端资产不完整")


def publish_immutable(github, tag, commit, paths, notes):
    paths = list(paths)
    expected = {safe_name(p.name): "sha256:" + digest(p) for p in paths}
    if len(expected) != len(paths):
        raise ValueError("发布资产名称重复")
    tag_commit = github.get_tag(tag)
    release = github.get_release(tag)
    if tag_commit is not None and tag_commit != commit:
        raise ValueError("已有版本标签指向另一提交，拒绝移动")
    if release is not None:
        # tag 已存在时 target_commitish 是被忽略的创建提示；真实来源以
        # tag 解引用的 commit + 固定资产哈希判断，不拿分支提示替代来源。
        if tag_commit is None:
            raise ValueError("已有发布记录缺少固定版本标签，拒绝猜测来源")
        compare_assets(release, expected, complete=not release["draft"])
        if not release["draft"]:
            if tag_commit is None:
                raise ValueError("已发布版本缺少对应标签")
            return
    if tag_commit is None:
        github.create_tag(tag, commit)
    if release is None:
        github.create_draft(tag, commit, notes)
    github.upload(tag, paths)
    release = github.get_release(tag)
    if (release is None or not release.get("draft")
            or github.get_tag(tag) != commit):
        raise ValueError("上传后发布身份改变")
    compare_assets(release, expected, complete=True)
    github.publish(tag)
    release = github.get_release(tag)
    if release is None or release.get("draft") or github.get_tag(tag) != commit:
        raise ValueError("公开发布未完成或标签身份改变")
    compare_assets(release, expected, complete=True)


class Github:
    def __init__(self, repository):
        if not re.fullmatch(r"[\w.-]+/[\w.-]+", repository):
            raise ValueError("仓库名称非法")
        self.repository = repository
        self.api = "repos/" + repository

    def run(self, *args, optional=False):
        result = subprocess.run(["gh", *args], capture_output=True, encoding="utf-8", check=False)
        if result.returncode:
            if optional and "HTTP 404" in result.stderr:
                return None
            raise RuntimeError(f"GitHub 操作失败：{result.stderr.strip()}")
        return result.stdout

    def get_tag(self, tag):
        result = self.run("api", f"{self.api}/git/ref/tags/{tag}", optional=True)
        if result is None:
            return None
        # /commits 将 annotated/lightweight tag 都解引用到实际提交。
        return json.loads(self.run("api", f"{self.api}/commits/{tag}"))["sha"]

    def get_release(self, tag):
        result = self.run("api", f"{self.api}/releases/tags/{tag}", optional=True)
        if result is not None:
            return json.loads(result)
        # by-tag 文档只保证已发布版本；认证的 list 接口可读取本仓草稿。
        page = 1
        while True:
            releases = json.loads(self.run("api", f"{self.api}/releases?per_page=100&page={page}"))
            for release in releases:
                if release.get("tag_name") == tag:
                    return release
            if len(releases) < 100:
                return None
            page += 1

    def create_tag(self, tag, sha):
        self.run("api", "--method", "POST", f"{self.api}/git/refs",
                 "-f", "ref=refs/tags/" + tag, "-f", "sha=" + sha)

    def create_draft(self, tag, sha, notes):
        with tempfile.TemporaryDirectory(prefix="tokenscope-release-notes-") as temporary:
            path = Path(temporary) / "notes.md"
            path.write_text(notes, encoding="utf-8")
            self.run("release", "create", tag, "--repo", self.repository, "--draft",
                     "--target", "main", "--verify-tag", "--title", "TokenScope " + tag,
                     "--prerelease", "--latest=false", "--notes-file", str(path))

    def upload(self, tag, paths):
        existing = {a["name"] for a in self.get_release(tag)["assets"]}
        missing = [str(p) for p in paths if p.name not in existing]
        if missing:
            self.run("release", "upload", tag, *missing, "--repo", self.repository)

    def publish(self, tag):
        self.run("release", "edit", tag, "--repo", self.repository,
                 "--draft=false", "--prerelease", "--latest=false")


def publish(directory, commit, version):
    metadata = verify_current(directory, commit, version)
    directory = Path(directory)
    provenance = directory / "provenance.sigstore.json"
    if not provenance.is_file() or not provenance.stat().st_size:
        raise ValueError("当前资产缺少 Sigstore 来源证明")
    baseline = load_baseline()
    _, historical = historical_versions(ROOT, baseline["commit"])
    if historical != baseline["version"]:
        raise ValueError("历史提交的四份版本配置不一致")
    subprocess.run(["git", "-C", str(ROOT), "merge-base", "--is-ancestor",
                    baseline["commit"], commit], check=True)
    github = Github(os.environ["GITHUB_REPOSITORY"])
    with tempfile.TemporaryDirectory(prefix="tokenscope-history-release-") as temporary:
        old_paths = download_baseline(temporary, baseline)
        old_notes = (f"历史版本身份回补：{baseline['version']}。\n\n"
                     f"Source commit: {baseline['commit']}\n"
                     f"Original release: {baseline['origin_release']}\n\n"
                     "复用原安装包、校验和与来源证明的相同字节和原文件名，未重新构建。\n"
                     "沿用原发布时的验收边界；当前版本的升级证据在后续 Release 中记录。\n")
        publish_immutable(github, baseline["tag"], baseline["commit"], old_paths, old_notes)
    paths = [*directory.glob("*-setup.exe"), directory / "SHA256SUMS.txt",
             directory / "build-metadata.json", provenance]
    notes = (f"TokenScope {version}\n\n{declaration(ROOT)['summary']}\n\nSource commit: {commit}\n"
             f"Build and checks: {os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/"
             f"{os.environ['GITHUB_REPOSITORY']}/actions/runs/{os.environ['GITHUB_RUN_ID']}\n\n"
             "Windows x64 NSIS installer; checksums, build metadata and Sigstore provenance attached.\n"
             f"Installed executable SHA256: {metadata['executable_sha256']}\n"
             "The installer is not Authenticode-signed.\n")
    publish_immutable(github, "v" + version, commit, paths, notes)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    for mode in ("verify", "publish"):
        command = sub.add_parser(mode)
        command.add_argument("--directory", type=Path, required=True)
        command.add_argument("--commit", required=True)
        command.add_argument("--version", required=True)
    command = sub.add_parser("build-metadata")
    command.add_argument("--executable", type=Path, required=True)
    command.add_argument("--commit", required=True)
    command.add_argument("--output", type=Path, required=True)
    command = sub.add_parser("checksums")
    command.add_argument("--directory", type=Path, required=True)
    command.add_argument("files", nargs="+")
    command = sub.add_parser("download-baseline")
    command.add_argument("--directory", type=Path, required=True)
    command = sub.add_parser("collect")
    command.add_argument("--bundle", type=Path, required=True)
    command.add_argument("--executable", type=Path, required=True)
    command.add_argument("--directory", type=Path, required=True)
    command.add_argument("--commit", required=True)
    command.add_argument("--version", required=True)
    args = parser.parse_args()
    try:
        if args.mode == "build-metadata":
            result = build_metadata(args.executable, args.commit, args.output)
        elif args.mode == "checksums":
            write_checksums(args.directory, [args.directory / safe_name(n) for n in args.files])
            result = {"ok": True}
        elif args.mode == "download-baseline":
            paths = download_baseline(args.directory)
            result = {"installer": str(next(p for p in paths if p.name.endswith("-setup.exe")))}
        elif args.mode == "verify":
            result = verify_current(args.directory, args.commit, args.version)
        elif args.mode == "collect":
            result = collect(args.bundle,args.executable,args.directory,args.commit,args.version)
        else:
            publish(args.directory, args.commit, args.version)
            result = {"published": "v" + args.version, "commit": args.commit}
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0
    except (ValueError, OSError, KeyError, RuntimeError, subprocess.CalledProcessError) as exc:
        print(f"发布资产校验失败：{exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    sys.exit(main())
