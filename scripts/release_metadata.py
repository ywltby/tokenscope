"""Validate release identity without modifying application versions or tags."""

import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

SEMVER = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-((?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)


def metadata(ref, sha, versions):
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("Expected a full commit SHA")
    if len(set(versions.values())) != 1:
        raise ValueError(f"Application versions disagree: {versions}")
    version = next(iter(versions.values()))
    if not isinstance(version, str) or not SEMVER.fullmatch(version):
        raise ValueError("Application version must be SemVer")
    if ref == "refs/heads/main":
        tag = f"pre-{sha}"
        prerelease = True
        title = f"TokenScope {version} main ({sha[:12]})"
    elif ref.startswith("refs/tags/v"):
        tag = ref.removeprefix("refs/tags/")
        match = SEMVER.fullmatch(tag[1:])
        if match is None or tag[1:] != version:
            raise ValueError("Version tag must exactly match all application versions")
        prerelease = match.group(4) is not None
        title = f"TokenScope {tag}"
    else:
        raise ValueError("Only main and v-prefixed SemVer tags can publish")
    return {
        "tag": tag,
        "sha": sha,
        "version": version,
        "title": title,
        "prerelease": str(prerelease).lower(),
        "latest": "false" if prerelease else "true",
    }


def read_versions(root):
    versions = {}
    for name in ("Cargo.toml", "src-tauri/Cargo.toml"):
        versions[name] = tomllib.loads((root / name).read_text(encoding="utf-8"))["package"]["version"]
    for name in ("frontend/package.json", "src-tauri/tauri.conf.json"):
        versions[name] = json.loads((root / name).read_text(encoding="utf-8"))["version"]
    return versions


def main():
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    # github.sha may identify an annotated tag; compare its peeled commit instead.
    expected = subprocess.check_output(
        ["git", "rev-parse", os.environ["GITHUB_SHA"] + "^{commit}"], text=True
    ).strip()
    if sha != expected:
        raise ValueError("Checkout does not match the triggering commit")
    ref = os.environ["GITHUB_REF"]
    if ref.startswith("refs/tags/"):
        subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], check=True)
    result = metadata(ref, sha, read_versions(Path.cwd()))
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for key, value in result.items():
            output.write(f"{key}={value}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
