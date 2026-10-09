import unittest

from release_metadata import metadata, read_versions
from pathlib import Path

SHA = "a" * 40


class ReleaseMetadataTests(unittest.TestCase):
    def make(self, ref, version="0.1.0", sha=SHA):
        return metadata(ref, sha, {"app": version, "frontend": version})

    def test_main_is_unique_prerelease_never_latest(self):
        result = self.make("refs/heads/main")
        self.assertEqual(result["tag"], "pre-" + SHA)
        self.assertEqual(result["prerelease"], "true")
        self.assertEqual(result["latest"], "false")
        self.assertNotEqual(result["tag"], self.make("refs/heads/main", sha="b" * 40)["tag"])

    def test_stable_tag_is_latest(self):
        result = self.make("refs/tags/v0.1.0")
        self.assertEqual(result["prerelease"], "false")
        self.assertEqual(result["latest"], "true")

    def test_prerelease_version_tags_stay_prereleases(self):
        for version in ("0.1.0-rc.1", "1.0.0-0", "1.0.0-alpha.2+build.5"):
            with self.subTest(version=version):
                result = self.make("refs/tags/v" + version, version)
                self.assertEqual(result["prerelease"], "true")
                self.assertEqual(result["latest"], "false")

    def test_rejects_non_release_refs(self):
        for ref in ("refs/heads/feature", "refs/tags/pre-" + SHA, "refs/pull/1/merge"):
            with self.subTest(ref=ref), self.assertRaises(ValueError):
                self.make(ref)

    def test_rejects_invalid_semver_and_mismatches(self):
        for version in ("01.0.0", "1.0", "1.0.0-01", "1.0.0-", "1.0.0+", "1.0.0\n"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                self.make("refs/tags/v" + version, version)
        with self.assertRaises(ValueError):
            self.make("refs/tags/v0.2.0")
        with self.assertRaises(ValueError):
            metadata("refs/heads/main", SHA, {"app": "0.1.0", "frontend": "0.2.0"})

    def test_rejects_partial_sha(self):
        with self.assertRaises(ValueError):
            self.make("refs/heads/main", sha="abc123")

    def test_repository_versions_agree(self):
        versions = read_versions(Path(__file__).resolve().parent.parent)
        self.assertEqual(len(versions), 4)
        self.assertEqual(len(set(versions.values())), 1)


if __name__ == "__main__":
    unittest.main()
