"""展示名目录生成器：显式 canonical 关系的保留与排除规则（合成输入）。

末段相同的显式别名（relay/demo-v2 -> publisher/demo-v2）是发布方的身份
关系，生成器不得丢弃；解析侧再按“完整 ID 自身规范名优先”决定是否使用。
"""

import unittest

from update_model_display_catalog import build


def models(*entries):
    return {entry["id"]: entry for entry in entries}


def api(**providers):
    return {provider: {"models": body} for provider, body in providers.items()}


class BuildCatalogCase(unittest.TestCase):
    def test_same_leaf_explicit_alias_is_kept(self):
        catalog = build(
            models(
                {"id": "publisher/demo-v2", "name": "Demo V2"},
                {"id": "publisher/demo-v3", "name": "Demo V3"},
            ),
            api(
                relay={
                    "demo-v2": {"canonical_model_id": "publisher/demo-v2"},
                    "demo-v3": {"canonical_model_id": "publisher/demo-v3"},
                }
            ),
            "2026-11-25",
        )
        self.assertEqual(
            catalog["aliases"],
            [
                {"id": "relay/demo-v2", "canonical_id": "publisher/demo-v2"},
                {"id": "relay/demo-v3", "canonical_id": "publisher/demo-v3"},
            ],
        )
        self.assertNotIn("且末段不同", catalog["source"]["note"])

    def test_two_publishers_with_different_models_keep_distinct_entries(self):
        catalog = build(
            models(
                {"id": "author-a/demo-x", "name": "Demo A One"},
                {"id": "author-b/demo-x", "name": "Demo B Two"},
            ),
            api(relay={"demo-x": {"canonical_model_id": "author-a/demo-x"}}),
            "2026-11-25",
        )
        self.assertEqual(
            catalog["models"],
            [
                {"id": "author-a/demo-x", "name": "Demo A One"},
                {"id": "author-b/demo-x", "name": "Demo B Two"},
            ],
        )
        self.assertEqual(
            catalog["aliases"],
            [{"id": "relay/demo-x", "canonical_id": "author-a/demo-x"}],
        )

    def test_alias_requires_canonical_target_and_not_self(self):
        catalog = build(
            models({"id": "publisher/demo-v2", "name": "Demo V2"}),
            api(
                relay={
                    "demo-v2": {"canonical_model_id": "publisher/demo-v2"},
                    "self": {"canonical_model_id": "relay/self"},
                    "missing": {"canonical_model_id": "publisher/not-in-models-json"},
                    "bad-type": {"canonical_model_id": 42},
                    "absent": {},
                }
            ),
            "2026-11-25",
        )
        self.assertEqual(
            catalog["aliases"],
            [{"id": "relay/demo-v2", "canonical_id": "publisher/demo-v2"}],
        )

    def test_output_is_sorted_and_order_independent(self):
        models_forward = models(
            {"id": "publisher/demo-v2", "name": "Demo V2"},
            {"id": "publisher/other", "name": "Other"},
        )
        models_reverse = dict(reversed(list(models_forward.items())))
        api_forward = api(
            relay={
                "demo-v2": {"canonical_model_id": "publisher/demo-v2"},
                "other": {"canonical_model_id": "publisher/other"},
            }
        )
        api_reverse = {
            "relay": {
                "models": dict(
                    reversed(list(api_forward["relay"]["models"].items()))
                )
            }
        }
        first = build(models_forward, api_forward, "2026-11-25")
        second = build(models_reverse, api_reverse, "2026-11-25")
        self.assertEqual(first, second)
        ids = [alias["id"] for alias in first["aliases"]]
        self.assertEqual(ids, sorted(ids))


if __name__ == "__main__":
    unittest.main()