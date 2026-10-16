"""升级起点登记表与选择规则的契约测试：全部离线，远端读取用替身。"""

import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import upgrade_baselines as baselines

SHA = "a" * 40
FINGERPRINT = {"cache_schema": "11", "history_schema": "4", "parser_revision": "1",
               "identity_revision": "1", "pricing_index": "9", "settings_format": "1"}


def release(tag, assets, commit=SHA, draft=False):
    return {"tag_name": tag, "draft": draft, "assets": assets}


def asset(name, digest=None, tag="v0.3.2"):
    value = {"name": name,
             "browser_download_url": f"https://github.com/ywltby/tokenscope/releases/download/{tag}/{name}"}
    if digest is not None:
        value["digest"] = "sha256:" + digest
    return value


class FakeApi:
    def __init__(self, releases=None, commits=None):
        self.releases = releases or {}
        self.commits = commits or {}

    def release(self, tag):
        return self.releases.get(tag)

    def tag_commit(self, tag):
        return self.commits.get(tag)


class RegistryCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-baselines-")
        self.addCleanup(self.temp.cleanup)
        self.registry = baselines.load_registry()

    def write(self, payload):
        path = Path(self.temp.name) / "upgrade_baselines.json"
        path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        return path

    def without(self, *versions):
        registry = copy.deepcopy(self.registry)
        registry["entries"] = [e for e in registry["entries"] if e["version"] not in versions]
        return registry

    def with_entry(self, version, **changes):
        registry = copy.deepcopy(self.registry)
        for entry in registry["entries"]:
            if entry["version"] == version:
                entry.update(changes)
                return registry
        raise AssertionError(f"登记表没有 {version}")


class SelectionCase(RegistryCase):
    def test_published_registry_selects_baseline_and_previous_version(self):
        selected = baselines.select_origins(self.registry, "0.3.3")
        self.assertEqual([e["version"] for e in selected], ["0.1.0", "0.3.2"])
        payload = baselines.selection_payload(self.registry, "0.3.3")
        self.assertEqual(payload["origins"], ["0.1.0", "0.3.2"])
        self.assertEqual(payload["include"], [
            {"origin": "0.1.0", "source_state": "present"},
            {"origin": "0.1.0", "source_state": "deleted"},
            {"origin": "0.3.2", "source_state": "present"},
            {"origin": "0.3.2", "source_state": "deleted"}])
        self.assertEqual(payload["chain"], payload["origins"])

    def test_before_previous_version_is_registered_selection_falls_back(self):
        registry = self.without("0.3.2")
        self.assertEqual([e["version"] for e in baselines.select_origins(registry, "0.3.2")],
                         ["0.1.0", "0.3.1"])
        self.assertEqual([e["version"] for e in baselines.select_origins(registry, "0.3.3")],
                         ["0.1.0", "0.3.1"])

    def test_withdrawn_and_unpublished_versions_are_never_origins(self):
        registry = self.with_entry("0.3.2", status="withdrawn",
                                   withdrawn={"date": "2026-10-20", "reason": "崩溃"})
        registry["entries"].append({"version": "0.3.3", "tag": "v0.3.3", "commit": SHA,
                                    "origin_release": "v0.3.3", "profile": "history",
                                    "status": "unpublished", "assets": []})
        self.assertEqual([e["version"] for e in baselines.select_origins(registry, "0.3.4")],
                         ["0.1.0", "0.3.1"])

    def test_each_persistent_format_keeps_its_own_latest_representative(self):
        registry = self.with_entry("0.2.0", persistent_format={**FINGERPRINT, "history_schema": "3"})
        selected = [e["version"] for e in baselines.select_origins(registry, "0.3.3")]
        self.assertEqual(selected, ["0.1.0", "0.2.0", "0.3.2"])

    def test_two_versions_of_one_format_are_not_treated_as_separate_groups(self):
        registry = self.with_entry("0.2.1", persistent_format={**FINGERPRINT, "history_schema": "3"})
        registry = copy.deepcopy(registry)
        selected = [e["version"] for e in baselines.select_origins(registry, "0.3.3")]
        self.assertEqual(selected, ["0.1.0", "0.2.1", "0.3.2"])

    def test_missing_or_withdrawn_baseline_requires_review(self):
        for registry in (self.without("0.1.0"),
                         self.with_entry("0.1.0", status="withdrawn",
                                         withdrawn={"date": "2026-10-20", "reason": "崩溃"})):
            with self.subTest(registry=registry["entries"][0]["version"]):
                with self.assertRaises(ValueError):
                    baselines.select_origins(registry, "0.3.3")

    def test_too_many_origins_stop_instead_of_expanding_silently(self):
        registry = self.without("0.3.2")
        for index, version in enumerate(("0.3.2", "0.3.3", "0.3.4", "0.3.5", "0.3.6")):
            registry["entries"].append({
                "version": version, "tag": "v" + version, "commit": SHA,
                "origin_release": "v" + version, "profile": "history", "status": "published",
                "persistent_format": {**FINGERPRINT, "history_schema": str(10 + index)},
                "assets": [{"name": f"TokenScope_v{version}_windows-x64-setup.exe",
                            "url": "https://github.com/ywltby/tokenscope/releases/download/v"
                                   f"{version}/TokenScope_v{version}_windows-x64-setup.exe",
                            "sha256": "0" * 64}]})
        self.assertEqual(len(baselines.select_origins(registry, "0.3.6")), 6)
        with self.assertRaises(ValueError):
            baselines.select_origins(registry, "0.3.7")


class RegistryValidationCase(RegistryCase):
    def payload(self):
        return copy.deepcopy(self.registry)

    def assert_rejected(self, payload):
        with self.assertRaises(ValueError):
            baselines.load_registry(self.write(payload))

    def test_broken_registry_structures_are_rejected(self):
        payload = self.payload()
        payload["schema_version"] = 1
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["assets"][0]["sha256"] = "not-a-digest"
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["assets"][0]["url"] = "https://example.invalid/setup.exe"
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["assets"] = payload["entries"][1]["assets"][:2]
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["assets"].append(copy.deepcopy(payload["entries"][1]["assets"][0]))
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["profile"] = "legacy-cache"
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["status"] = "withdrawn"
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["persistent_format"].pop("identity_revision")
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["persistent_format"]["history_schema"] = 4
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["expected_after"] = {"present": "scripts/fixtures/x.json"}
        self.assert_rejected(payload)
        payload = self.payload()
        payload["entries"][1]["commit"] = "short"
        self.assert_rejected(payload)

    def test_unpublished_entry_carries_commit_and_reason_without_assets(self):
        payload = self.payload()
        payload["entries"].append({"version": "0.3.3", "tag": "v0.3.3", "commit": SHA,
                                   "origin_release": "v0.3.3", "profile": "history",
                                   "status": "unpublished", "assets": []})
        path = self.write(payload)
        registry = baselines.load_registry(path)
        self.assertEqual(registry["entries"][-1]["status"], "unpublished")

    def test_resolved_entry_exposes_manifest_paths_and_installer(self):
        view = baselines.resolved_entry(baselines.published_entry(self.registry, "0.2.0"))
        self.assertEqual(view["expected_old"], str(baselines.ROOT / baselines.DEFAULT_EXPECTED))
        self.assertEqual(view["expected_after"]["present"], view["expected_old"])
        self.assertEqual(view["expected_after"]["deleted"], view["expected_old"])
        self.assertEqual(view["installer_name"], "TokenScope_v0.2.0_windows-x64-setup.exe")
        self.assertEqual(view["installer_sha256"],
                         "2a84dad613481b28ba09b0c820eba11875ee93ee5b6afb20d34bd3b7fcccbb9c")


class CheckCase(RegistryCase):
    def setUp(self):
        super().setUp()
        self.work = Path(self.temp.name) / "downloads"
        self.work.mkdir()
        self.metadata_file = self.work / "build-metadata.json"

    def api_for_registry(self, registry, *, commit_ok=True, drop=(), replace=()):
        releases, commits = {}, {}
        for entry in registry["entries"]:
            if entry["version"] in drop:
                continue
            assets = []
            for a in entry["assets"]:
                digest = "0" * 64 if a["name"] in replace else a["sha256"]
                assets.append(asset(a["name"], digest))
            releases[entry["tag"]] = release(entry["tag"], assets)
            commits[entry["tag"]] = entry["commit"] if commit_ok else SHA
        return FakeApi(releases, commits)

    def run_check(self, registry, base="0.3.2", **kwargs):
        path = self.write(registry)
        api = self.api_for_registry(registry, **kwargs)
        with patch.object(baselines, "base_version", return_value=base), \
                patch.object(baselines, "download", return_value=(self.metadata_file, "0" * 64)):
            self.metadata_file.write_text(json.dumps({"schema_version": 1}), encoding="utf-8")
            return baselines.check_registry("origin/main", path=path, api=api)

    def test_missing_base_version_is_reported_with_the_add_command(self):
        registry = self.without("0.3.2")
        path = self.write(registry)
        with patch.object(baselines, "base_version", return_value="0.3.2"):
            with self.assertRaises(ValueError) as caught:
                baselines.check_registry("origin/main", path=path, api=self.api_for_registry(registry))
        self.assertIn("add --tag v0.3.2", str(caught.exception))

    def test_withdrawn_base_version_blocks_the_batch(self):
        registry = self.with_entry("0.3.2", status="withdrawn",
                                   withdrawn={"date": "2026-10-20", "reason": "崩溃"})
        path = self.write(registry)
        with patch.object(baselines, "base_version", return_value="0.3.2"):
            with self.assertRaises(ValueError):
                baselines.check_registry("origin/main", path=path,
                                         api=self.api_for_registry(registry))

    def test_deleted_release_or_moved_tag_fails_with_withdrawn_hint(self):
        for kwargs in ({"drop": ("0.3.1",)}, {"commit_ok": False}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError) as caught:
                self.run_check(self.registry, **kwargs)
            self.assertIn("withdrawn", str(caught.exception))

    def test_replaced_asset_digest_fails(self):
        with self.assertRaises(ValueError) as caught:
            self.run_check(self.registry, replace=("TokenScope_v0.3.1_windows-x64-setup.exe",))
        self.assertIn("被替换", str(caught.exception))

    def test_withdrawn_entry_with_existing_release_is_only_a_warning(self):
        registry = self.with_entry("0.3.1", status="withdrawn",
                                   withdrawn={"date": "2026-10-20", "reason": "崩溃"})
        result = self.run_check(registry)
        self.assertEqual(result["checked"], ["0.1.0", "0.2.0", "0.2.1", "0.3.0", "0.3.2"])
        self.assertTrue(any("0.3.1" in warning for warning in result["warnings"]))

    def test_metadata_fingerprint_must_match_the_registry(self):
        registry = copy.deepcopy(self.registry)
        api = self.api_for_registry(registry)
        path = self.write(registry)
        metadata = {"schema_version": 2, "persistent_format": {**FINGERPRINT, "settings_format": "2"}}

        def fake_download(url, sha256, directory, name):
            self.metadata_file.write_text(json.dumps(metadata), encoding="utf-8")
            return self.metadata_file, sha256 or "0" * 64

        with patch.object(baselines, "base_version", return_value="0.3.2"), \
                patch.object(baselines, "download", side_effect=fake_download):
            with self.assertRaises(ValueError) as caught:
                baselines.check_registry("origin/main", path=path, api=api)
        self.assertIn("持久格式", str(caught.exception))

    def test_github_errors_are_not_skipped(self):
        class Broken(FakeApi):
            def __init__(self, inner):
                super().__init__(inner.releases, inner.commits)

            def release(self, tag):
                raise RuntimeError("GitHub 读取失败：HTTP 500")

        path = self.write(self.registry)
        with patch.object(baselines, "base_version", return_value="0.3.2"):
            with self.assertRaises(RuntimeError):
                baselines.check_registry("origin/main", path=path,
                                         api=Broken(self.api_for_registry(self.registry)))


class AddCase(RegistryCase):
    def test_unpublished_release_waits_and_unpublished_status_needs_reason(self):
        registry = self.without("0.3.2")
        path = self.write(registry)
        with self.assertRaises(ValueError) as caught:
            baselines.add_entry("v0.3.2", api=FakeApi(), path=path)
        self.assertIn("等待 v0.3.2", str(caught.exception))
        with self.assertRaises(ValueError):
            baselines.add_entry("v0.3.2", api=FakeApi(), path=path, status="unpublished")
        entry = baselines.add_entry("v0.3.2", api=FakeApi(), path=path, status="unpublished",
                                    commit=SHA, reason="发布流水线失败")
        self.assertEqual(entry["status"], "unpublished")
        self.assertEqual(baselines.load_registry(path)["entries"][-1]["version"], "0.3.2")

    def test_schema_one_metadata_requires_manual_registration(self):
        registry = self.without("0.3.2")
        path = self.write(registry)
        assets = [asset("TokenScope_v0.3.2_windows-x64-setup.exe", "1" * 64),
                  asset("SHA256SUMS.txt", "2" * 64),
                  asset("provenance.sigstore.json", "3" * 64),
                  asset("build-metadata.json", "4" * 64)]
        api = FakeApi({"v0.3.2": release("v0.3.2", assets, commit=SHA)}, {"v0.3.2": SHA})
        metadata = self.work_dir() / "build-metadata.json"

        def fake_download(url, sha256, directory, name):
            path_ = Path(directory) / name
            if name == "build-metadata.json":
                path_.write_text(json.dumps({"schema_version": 1, "version": "0.3.2"}),
                                 encoding="utf-8")
            else:
                path_.write_bytes(name.encode("utf-8"))
            return path_, "1" * 64

        with patch.object(baselines, "download", side_effect=fake_download):
            with self.assertRaises(ValueError) as caught:
                baselines.add_entry("v0.3.2", api=api, path=path)
        self.assertIn("persistent_format", str(caught.exception))
        self.assertEqual(len(baselines.load_registry(path)["entries"]), 5)

    def work_dir(self):
        return Path(self.temp.name)

    def test_add_registers_downloaded_assets_and_metadata(self):
        registry = self.without("0.3.2")
        path = self.write(registry)
        names = ["TokenScope_v0.3.2_windows-x64-setup.exe", "SHA256SUMS.txt",
                 "provenance.sigstore.json", "build-metadata.json"]
        api = FakeApi({"v0.3.2": release("v0.3.2", [asset(n, "5" * 64) for n in names],
                                        commit=SHA)}, {"v0.3.2": SHA})

        def fake_download(url, sha256, directory, name):
            path_ = Path(directory) / name
            if name == "build-metadata.json":
                path_.write_text(json.dumps({"schema_version": 2, "persistent_format": FINGERPRINT}),
                                 encoding="utf-8")
            else:
                path_.write_bytes(name.encode("utf-8"))
            return path_, "5" * 64

        with patch.object(baselines, "download", side_effect=fake_download):
            with patch.object(baselines, "digest", return_value="5" * 64):
                entry = baselines.add_entry("v0.3.2", api=api, path=path,
                                            directory=self.temp.name)
        self.assertEqual([a["sha256"] for a in entry["assets"]], ["5" * 64] * 4)
        self.assertEqual(entry["origin_release"], "v0.3.2")
        self.assertEqual(entry["profile"], "history")
        self.assertEqual(entry["persistent_format"], FINGERPRINT)
        stored = baselines.load_registry(path)
        self.assertEqual([e["version"] for e in stored["entries"]],
                         ["0.1.0", "0.2.0", "0.2.1", "0.3.0", "0.3.1", "0.3.2"])


class WithdrawCase(RegistryCase):
    def test_withdraw_marks_the_entry_and_removes_it_from_selection(self):
        registry = self.without("0.3.2")
        path = self.write(registry)
        entry = baselines.withdraw_entry("0.3.1", "配置损坏", "2026-10-20", path=path)
        self.assertEqual(entry["status"], "withdrawn")
        self.assertEqual(entry["withdrawn"]["date"], "2026-10-20")
        stored = baselines.load_registry(path)
        self.assertEqual([e["version"] for e in baselines.select_origins(stored, "0.3.3")],
                         ["0.1.0", "0.3.0"])
        with self.assertRaises(ValueError):
            baselines.withdraw_entry("0.3.1", "", "2026-10-20", path=path)
        with self.assertRaises(ValueError):
            baselines.withdraw_entry("9.9.9", "配置损坏", "2026-10-20", path=path)


class RepositoryCase(unittest.TestCase):
    def test_repository_is_read_from_the_registry_and_tokens_are_optional(self):
        registry = baselines.load_registry()
        self.assertEqual(registry["repository"], "ywltby/tokenscope")
        with patch.dict(os.environ, {"GH_TOKEN": "", "GITHUB_TOKEN": ""}):
            api = baselines.Api(registry["repository"])
        self.assertIsNone(api.token)
        with patch.dict(os.environ, {"GITHUB_TOKEN": "token"}):
            self.assertEqual(baselines.Api(registry["repository"]).token, "token")


if __name__ == "__main__":
    unittest.main()
