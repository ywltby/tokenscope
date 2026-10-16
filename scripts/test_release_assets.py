"""对真实临时资产和受控 GitHub API 验证不可变发布，永不写远端。"""

import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import release_assets as assets

SHA = "a" * 40
FINGERPRINT = {"cache_schema": "11", "history_schema": "4", "parser_revision": "1",
               "identity_revision": "1", "pricing_index": "9", "settings_format": "1"}


def legacy_registry(status="published"):
    entry = {"version": "0.1.0", "tag": "v0.1.0", "commit": SHA, "origin_release": "pre-" + SHA,
             "profile": "legacy-cache", "status": status}
    if status == "withdrawn":
        entry["withdrawn"] = {"date": "2026-10-20", "reason": "崩溃"}
    return {"schema_version": 2, "repository": "ywltby/tokenscope", "entries": [entry]}


class FakeGithub:
    def __init__(self, release=None, tag=None):
        self.release = release
        self.tag = tag
        self.operations = []

    def get_tag(self, tag):
        return self.tag

    def get_release(self, tag):
        return self.release

    def create_tag(self, tag, sha):
        self.operations.append(("tag", tag, sha))
        self.tag = sha

    def create_draft(self, tag, sha, notes):
        self.operations.append(("draft", tag, sha))
        self.release = {"draft": True, "target_commitish": sha, "assets": []}

    def upload(self, tag, paths):
        self.operations.append(("upload", tuple(p.name for p in paths)))
        self.release["assets"] = [{"name": p.name, "digest": "sha256:" + assets.digest(p)}
                                  for p in paths]

    def publish(self, tag):
        self.operations.append(("publish", tag))
        self.release["draft"] = False


class ReleaseAssetCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-release-")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.exe = self.directory / "TokenScope_v0.2.0_windows-x64-setup.exe"
        self.exe.write_bytes(b"inert installer, never executed")
        self.metadata = self.directory / "build-metadata.json"
        self.metadata.write_text(json.dumps({"schema_version": 2, "version": "0.2.0",
                                             "commit": SHA, "executable_sha256": "b" * 64,
                                             "persistent_format": FINGERPRINT}),
                                 encoding="utf-8")

    def checksum(self):
        assets.write_checksums(self.directory, [self.exe, self.metadata])

    def fingerprint_file(self):
        path = self.directory / "persistent-format.json"
        path.write_text(json.dumps(FINGERPRINT), encoding="utf-8")
        return path

    def test_checksums_bind_exact_files_and_build_identity(self):
        self.checksum()
        result = assets.verify_current(self.directory, SHA, "0.2.0")
        self.assertEqual(result["executable_sha256"], "b" * 64)
        self.exe.write_bytes(b"changed installer")
        with self.assertRaises(ValueError):
            assets.verify_current(self.directory, SHA, "0.2.0")

    def test_wrong_source_version_hash_missing_duplicate_or_traversal_entries_fail(self):
        self.checksum()
        original = (self.directory / "SHA256SUMS.txt").read_text(encoding="utf-8")
        for sha, version in (("c" * 40, "0.2.0"), (SHA, "0.2.1")):
            with self.subTest(sha=sha, version=version), self.assertRaises(ValueError):
                assets.verify_current(self.directory, sha, version)
        for content in (original.splitlines()[0] + "\n", original + original,
                        "a" * 64 + "  ../outside.exe\n", "not a digest\n"):
            with self.subTest(content=content):
                (self.directory / "SHA256SUMS.txt").write_text(content, encoding="utf-8")
                with self.assertRaises(ValueError):
                    assets.verify_current(self.directory, SHA, "0.2.0")
        (self.directory / "SHA256SUMS.txt").write_text(original, encoding="utf-8")
        payload = json.loads(self.metadata.read_text(encoding="utf-8"))
        payload["executable_sha256"] = "bad"
        self.metadata.write_text(json.dumps(payload), encoding="utf-8")
        self.checksum()
        with self.assertRaises(ValueError):
            assets.verify_current(self.directory, SHA, "0.2.0")

    def test_published_identity_is_idempotent_and_never_uploaded_again(self):
        paths = [self.exe, self.metadata]
        release = {"draft": False, "target_commitish": SHA,
                   "assets": [{"name": p.name, "digest": "sha256:" + assets.digest(p)}
                              for p in paths]}
        github = FakeGithub(release, SHA)
        assets.publish_immutable(github, "v0.2.0", SHA, paths, "notes")
        self.assertEqual(github.operations, [])
        for field in ("tag", "hash", "extra", "missing"):
            damaged = json.loads(json.dumps(release))
            tag = SHA
            if field == "tag":
                tag = "c" * 40
            elif field == "hash":
                damaged["assets"][0]["digest"] = "sha256:" + "0" * 64
            elif field == "extra":
                damaged["assets"].append({"name": "extra.exe", "digest": "sha256:" + "0" * 64})
            else:
                damaged["assets"].pop()
            github = FakeGithub(damaged, tag)
            with self.subTest(field=field), self.assertRaises(ValueError):
                assets.publish_immutable(github, "v0.2.0", SHA, paths, "notes")
            self.assertEqual(github.operations, [])

    def test_new_release_publishes_only_after_asset_identity_is_verified(self):
        github = FakeGithub()
        assets.publish_immutable(github, "v0.2.0", SHA, [self.exe, self.metadata], "notes")
        self.assertEqual([x[0] for x in github.operations], ["tag", "draft", "upload", "publish"])
        self.assertFalse(github.release["draft"])

    def test_wrong_uploaded_digest_is_never_published(self):
        class DamagedGithub(FakeGithub):
            def upload(self, tag, paths):
                super().upload(tag, paths)
                self.release["assets"][0]["digest"] = "sha256:" + "0" * 64

        github = DamagedGithub()
        with self.assertRaises(ValueError):
            assets.publish_immutable(github, "v0.2.0", SHA, [self.exe], "notes")
        self.assertTrue(github.release["draft"])
        self.assertNotIn("publish", [x[0] for x in github.operations])

    def test_resume_draft_accepts_only_expected_existing_assets(self):
        draft = {"draft": True, "target_commitish": SHA, "assets": []}
        github = FakeGithub(draft, SHA)
        assets.publish_immutable(github, "v0.2.0", SHA, [self.exe], "notes")
        self.assertEqual([x[0] for x in github.operations], ["upload", "publish"])
        github = FakeGithub({"draft": True, "target_commitish": SHA,
                             "assets": [{"name": "other.exe", "digest": "sha256:" + "0" * 64}]}, SHA)
        with self.assertRaises(ValueError):
            assets.publish_immutable(github, "v0.2.0", SHA, [self.exe], "notes")
        self.assertEqual(github.operations, [])

    def test_existing_tag_is_identity_and_release_target_is_only_creation_hint(self):
        release={"draft":False,"target_commitish":"main",
                 "assets":[{"name":self.exe.name,"digest":"sha256:"+assets.digest(self.exe)}]}
        github=FakeGithub(release,SHA)
        assets.publish_immutable(github,"v0.2.0",SHA,[self.exe],"notes")
        self.assertEqual(github.operations,[])
        github=FakeGithub(release,None)
        with self.assertRaises(ValueError):
            assets.publish_immutable(github,"v0.2.0",SHA,[self.exe],"notes")
        self.assertEqual(github.operations,[])

    def test_github_protocol_finds_drafts_and_creates_release_from_already_fixed_tag(self):
        github=assets.Github("ywltby/tokenscope")
        draft={"id":123,"tag_name":"v0.1.0","draft":True,"target_commitish":"main","assets":[]}
        calls=[]

        def response(*args,optional=False):
            calls.append(args)
            if args[1].endswith("/releases/tags/v0.1.0"):
                return None
            if args[1].endswith("&page=1"):
                return json.dumps([{"tag_name":"other"}]*100)
            return json.dumps([draft])

        with patch.object(github,"run",side_effect=response):
            self.assertEqual(github.get_release("v0.1.0"),draft)
        self.assertEqual(len(calls),3)
        with patch.object(github,"run",return_value=json.dumps(draft)) as run, patch.object(github,"get_tag",return_value=SHA):
            github.create_draft("v0.1.0",SHA,"historical notes")
        args=run.call_args.args
        self.assertIn("POST",args)
        self.assertEqual(args[0],"api")

    def test_created_draft_uses_returned_id_without_eventually_consistent_tag_or_list(self):
        github=assets.Github("ywltby/tokenscope")
        draft={"id":123,"tag_name":"v0.2.1","draft":True,"assets":[]}
        calls=[]
        def run(*args,**kwargs):
            calls.append(args)
            endpoint=args[1] if args[1].startswith("repos/") else args[args.index("POST")+1]
            if "POST" in args:
                with open(args[args.index("--input")+1],encoding="utf-8") as fh:
                    request=json.load(fh)
                self.assertEqual(request["tag_name"],"v0.2.1")
                self.assertEqual(request["target_commitish"],"main")
                self.assertEqual(request["body"],"本次发布")
                return json.dumps(draft)
            if endpoint.endswith("/releases/123"):
                return json.dumps(draft)
            self.fail("新建草稿不应重新依赖 by-tag 或 list")
        with patch.object(github,"run",side_effect=run),patch.object(github,"get_tag",return_value=SHA):
            github.create_draft("v0.2.1",SHA,"本次发布")
            self.assertEqual(github.get_release("v0.2.1"),draft)
        self.assertEqual(len(calls),2)

    def test_exact_release_id_is_retained_for_upload_and_publication(self):
        github=assets.Github("ywltby/tokenscope")
        draft={"id":123,"tag_name":"v0.2.1","draft":True,"assets":[]}
        calls=[]
        def run(*args,**kwargs):
            calls.append(args)
            if "uploads.github.com" in " ".join(args):
                self.assertIn("/releases/123/assets?name="+self.exe.name," ".join(args))
                self.assertEqual(args[args.index("--input")+1],str(self.exe))
                self.assertIn("Content-Type: application/octet-stream",args)
                return json.dumps({"name":self.exe.name,"digest":"sha256:"+assets.digest(self.exe)})
            if "PATCH" in args:
                self.assertIn("repos/ywltby/tokenscope/releases/123",args)
                return json.dumps({**draft,"draft":False})
            return json.dumps(draft)
        with patch.object(github,"run",side_effect=run):
            github.get_release("v0.2.1")
            github.upload("v0.2.1",[self.exe])
            github.publish("v0.2.1")
        self.assertTrue(any("POST" in args for args in calls))
        self.assertTrue(any("PATCH" in args for args in calls))

    def test_release_identity_change_is_rejected_before_mutation(self):
        github=assets.Github("ywltby/tokenscope")
        for release in ({"id":True,"tag_name":"v0.2.1"},
                        {"id":123,"tag_name":"v0.2.0"},
                        {"id":0,"tag_name":"v0.2.1"}):
            with self.subTest(release=release),self.assertRaises(ValueError):
                github.remember_release("v0.2.1",release)
        github.remember_release("v0.2.1",{"id":123,"tag_name":"v0.2.1"})
        with self.assertRaises(ValueError):
            github.remember_release("v0.2.1",{"id":456,"tag_name":"v0.2.1"})

    def test_baseline_pins_source_original_three_assets_and_expected_fixture(self):
        registry = assets.load_registry()
        baseline = assets.published_entry(registry, "0.1.0")
        self.assertEqual(baseline["profile"], "legacy-cache")
        self.assertEqual(baseline["persistent_format"]["cache_schema"], "6")
        self.assertEqual(baseline["commit"], "8d1d90de29aa00d10b9a7e4b8f906d909726446b")
        self.assertEqual(baseline["origin_release"], "pre-" + baseline["commit"])
        self.assertEqual(len(baseline["assets"]), 3)
        self.assertTrue(all(a["url"].startswith(
            "https://github.com/ywltby/tokenscope/releases/download/pre-" + baseline["commit"] + "/")
            for a in baseline["assets"]))
        self.assertEqual(assets.installer_of(baseline)["sha256"],
                         "798e7722120b414d2014cc80e987c831914f11d211abd8b5595f95184b62ece6")

    def test_publish_refuses_withdrawn_version_before_any_github_call(self):
        (self.directory / "provenance.sigstore.json").write_bytes(b"attestation bundle")
        registry = {"schema_version": 2, "repository": "ywltby/tokenscope", "entries": [
            {"version": "0.1.0", "tag": "v0.1.0", "commit": SHA, "origin_release": "v0.1.0",
             "profile": "legacy-cache", "status": "withdrawn",
             "withdrawn": {"date": "2026-10-20", "reason": "崩溃"},
             "assets": [{"name": "TokenScope_v0.1.0_windows-x64-setup.exe",
                         "url": "https://github.com/ywltby/tokenscope/releases/download/v0.1.0/"
                                "TokenScope_v0.1.0_windows-x64-setup.exe", "sha256": "0" * 64}]},
            {"version": "0.2.0", "tag": "v0.2.0", "commit": SHA, "origin_release": "v0.2.0",
             "profile": "history", "status": "withdrawn",
             "withdrawn": {"date": "2026-10-20", "reason": "崩溃"},
             "assets": []}]}
        with patch.object(assets, "verify_current", return_value={"executable_sha256": "b" * 64}), \
                patch.object(assets, "load_registry", return_value=registry), \
                patch.object(assets, "Github") as github, \
                patch.dict(os.environ, {"GITHUB_REPOSITORY": "ywltby/tokenscope"}):
            with self.assertRaises(ValueError):
                assets.publish(self.directory, SHA, "0.2.0")
        github.assert_not_called()

    def test_build_metadata_requires_complete_fingerprint(self):
        for value in (None, {}, {key: None for key in FINGERPRINT},
                      {key: FINGERPRINT[key] for key in list(FINGERPRINT)[:5]}):
            with self.subTest(value=value), self.assertRaises(ValueError):
                assets.validate_persistent_format(value)
        for value in ({**FINGERPRINT, "cache_schema": 11}, {**FINGERPRINT, "extra": "1"}):
            with self.subTest(value=value), self.assertRaises(ValueError):
                assets.validate_persistent_format(value)
        self.assertEqual(assets.read_persistent_format(self.fingerprint_file()), FINGERPRINT)

    def test_verify_current_rejects_metadata_without_fingerprint(self):
        for payload in ({"schema_version": 1, "version": "0.2.0", "commit": SHA,
                         "executable_sha256": "b" * 64},
                        {"schema_version": 2, "version": "0.2.0", "commit": SHA,
                         "executable_sha256": "b" * 64}):
            with self.subTest(payload=payload):
                self.metadata.write_text(json.dumps(payload), encoding="utf-8")
                self.checksum()
                with self.assertRaises(ValueError):
                    assets.verify_current(self.directory, SHA, "0.2.0")

    def test_collection_selects_current_version_even_with_stale_cached_installer(self):
        bundle = self.directory / "bundle"
        bundle.mkdir()
        (bundle / "TokenScope_0.1.0_x64-setup.exe").write_bytes(b"stale")
        (bundle / "TokenScope_0.2.0_x64-setup.exe").write_bytes(b"current")
        binary = self.directory / "tokenscope.exe"
        binary.write_bytes(b"current application")
        target = self.directory / "collected"
        bundled = self.directory / "bundled.exe"
        bundled.write_bytes(b"application patched for NSIS")
        with patch.object(assets, "resolve", return_value=SHA), patch.object(assets, "read_versions", return_value={"app":"0.2.0"}), patch.object(assets,"extract_payload",return_value=bundled,create=True) as extract:
            assets.collect(bundle,binary,target,SHA,"0.2.0",self.fingerprint_file())
        installer = next(target.glob("*-setup.exe"))
        self.assertEqual(installer.read_bytes(),b"current")
        metadata=assets.verify_current(target,SHA,"0.2.0")
        self.assertEqual(metadata["executable_sha256"],hashlib.sha256(bundled.read_bytes()).hexdigest())
        self.assertEqual(metadata["persistent_format"],FINGERPRINT)
        extract.assert_called_once()

    def test_payload_extraction_selects_one_root_binary_without_running_installer(self):
        directory=self.directory/"payload"
        commands=[]
        def run(args,**kwargs):
            commands.append(args)
            self.assertEqual(args[0],"controlled-7z")
            if args[1]=="l":
                return assets.subprocess.CompletedProcess(args,0,"Path = tokenscope.exe\nSize = 7\n","")
            self.assertEqual(args[1],"x")
            self.assertIn("-r-",args)
            (directory/"tokenscope.exe").write_bytes(b"bundled")
            return assets.subprocess.CompletedProcess(args,0,"","")
        with patch.object(assets.subprocess,"run",side_effect=run):
            payload=assets.extract_payload(self.exe,directory,command="controlled-7z")
        self.assertEqual(payload.read_bytes(),b"bundled")
        self.assertEqual(len(commands),2)

    def test_payload_extraction_rejects_missing_duplicate_traversal_and_failed_reader(self):
        for listing in ("", "Path = tokenscope.exe\nPath = tokenscope.exe\n",
                        "Path = ../tokenscope.exe\n", "Path = other/tokenscope.exe\n"):
            with self.subTest(listing=listing),tempfile.TemporaryDirectory() as destination:
                with patch.object(assets.subprocess,"run",return_value=assets.subprocess.CompletedProcess([],0,listing,"")) as run:
                    with self.assertRaises(ValueError):
                        assets.extract_payload(self.exe,destination,command="controlled-7z")
                    self.assertEqual(run.call_count,1)
        with patch.object(assets.subprocess,"run",return_value=assets.subprocess.CompletedProcess([],2,"","bad archive")):
            with self.assertRaises(RuntimeError):
                assets.extract_payload(self.exe,self.directory/"invalid",command="controlled-7z")

    def test_bad_download_hash_never_becomes_a_usable_old_package(self):
        import io
        entry = {"assets":[{"name":"old-setup.exe","url":"https://example.invalid/old",
                            "sha256":"0"*64}]}
        target = self.directory / "old"
        with patch.object(assets,"urlopen",return_value=io.BytesIO(b"wrong bytes")):
            with self.assertRaises(ValueError):
                assets.download_entry(entry,target)
        self.assertFalse((target / "old-setup.exe").exists())
        self.assertFalse(list(target.glob("*.download")))


if __name__ == "__main__":
    unittest.main()
