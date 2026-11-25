"""展示名目录生成器：显式 canonical 关系的保留与排除规则（合成输入）。

末段相同的显式别名（relay/demo-v2 -> publisher/demo-v2）是发布方的身份
关系，生成器不得丢弃；解析侧再按“完整 ID 自身规范名优先”决定是否使用。
"""

import unittest

from update_model_display_catalog import build, identity_key


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

    def test_history_layer_restores_only_evidenced_entries_and_current_upstream_wins(self):
        history = {
            "models": [{"id": "publisher/old", "name": "Old Name"}],
            "aliases": [
                {"id": "retired/old", "canonical_id": "publisher/old"},
                {"id": "relay/demo-v2", "canonical_id": "publisher/demo-v2"},
            ],
        }
        catalog = build(
            models(
                {"id": "publisher/old", "name": "Corrected Name"},
                {"id": "publisher/demo-v2", "name": "Demo V2"},
                {"id": "publisher/demo-v3", "name": "Demo V3"},
            ),
            api(relay={"demo-v2": {"canonical_model_id": "publisher/demo-v3"}}),
            "2026-11-25",
            history,
        )
        self.assertEqual(
            catalog["models"],
            [
                {"id": "publisher/demo-v2", "name": "Demo V2"},
                {"id": "publisher/demo-v3", "name": "Demo V3"},
                {"id": "publisher/old", "name": "Corrected Name"},
            ],
        )
        self.assertEqual(
            catalog["aliases"],
            [
                {"id": "relay/demo-v2", "canonical_id": "publisher/demo-v3"},
                {"id": "retired/old", "canonical_id": "publisher/old"},
            ],
        )
        self.assertEqual(catalog["source"]["history"]["models"], 0)
        self.assertEqual(catalog["source"]["history"]["aliases"], 1)
        reordered = {
            **history,
            "models": list(reversed(history["models"])),
            "aliases": list(reversed(history["aliases"])),
        }
        self.assertEqual(
            build(
                models(
                    {"id": "publisher/old", "name": "Corrected Name"},
                    {"id": "publisher/demo-v2", "name": "Demo V2"},
                    {"id": "publisher/demo-v3", "name": "Demo V3"},
                ),
                api(relay={"demo-v2": {"canonical_model_id": "publisher/demo-v3"}}),
                "2026-11-25",
                reordered,
            ),
            catalog,
        )

    def test_current_retarget_replaces_history_before_target_validation(self):
        catalog = build(
            models({"id": "publisher/current", "name": "Current"}),
            api(retired={"demo": {"canonical_model_id": "publisher/current"}}),
            "2026-11-25",
            {
                "aliases": [
                    {"id": "retired/demo", "canonical_id": "publisher/removed"}
                ]
            },
        )
        self.assertEqual(
            catalog["aliases"],
            [{"id": "retired/demo", "canonical_id": "publisher/current"}],
        )
        self.assertEqual(catalog["source"]["history"]["aliases"], 0)

    def test_history_layer_rejects_missing_targets_and_self_reference(self):
        for alias in [
            {"id": "retired/demo", "canonical_id": "publisher/missing"},
            {"id": "retired/demo", "canonical_id": "retired/demo"},
        ]:
            with self.subTest(alias=alias), self.assertRaises(ValueError):
                build(
                    models({"id": "publisher/demo", "name": "Demo"}),
                    api(),
                    "2026-11-25",
                    {"aliases": [alias]},
                )

    def test_present_entry_without_usable_mapping_does_not_fall_back_to_history(self):
        history = {
            "aliases": [
                {"id": "relay/model", "canonical_id": "publisher/old"},
                {"id": "retired/model", "canonical_id": "publisher/old"},
                {"id": "relay/retargeted", "canonical_id": "publisher/old"},
            ]
        }
        base_models = models(
            {"id": "publisher/old", "name": "Old Model"},
            {"id": "publisher/current", "name": "Current Model"},
        )
        counterexamples = [
            ("missing_canonical", {}),
            ("null_canonical", {"canonical_model_id": None}),
            ("self_reference", {"canonical_model_id": "relay/model"}),
            ("unlisted_target", {"canonical_model_id": "publisher/new"}),
        ]
        for label, entry_body in counterexamples:
            with self.subTest(case=label):
                forward_api = api(
                    relay={
                        "model": entry_body,
                        "retargeted": {"canonical_model_id": "publisher/current"},
                    }
                )
                catalog = build(base_models, forward_api, "2026-11-25", history)
                self.assertEqual(
                    catalog["aliases"],
                    [
                        {
                            "id": "relay/retargeted",
                            "canonical_id": "publisher/current",
                        },
                        {"id": "retired/model", "canonical_id": "publisher/old"},
                    ],
                )
                self.assertEqual(catalog["source"]["history"]["aliases"], 1)

                # 乱序对照：规范模型、当前 API 与历史别名全部反转后输出完全一致。
                reversed_models = dict(reversed(list(base_models.items())))
                reversed_api = {
                    "relay": {
                        "models": dict(
                            reversed(list(forward_api["relay"]["models"].items()))
                        )
                    }
                }
                reversed_history = {
                    "aliases": list(reversed(history["aliases"]))
                }
                self.assertEqual(
                    build(
                        reversed_models,
                        reversed_api,
                        "2026-11-25",
                        reversed_history,
                    ),
                    catalog,
                )

                # 现行条目已有自身规范名时，同样不输出历史别名，保留自身规范名条目。
                with_own_models = models(
                    {"id": "publisher/old", "name": "Old Model"},
                    {"id": "publisher/current", "name": "Current Model"},
                    {"id": "relay/model", "name": "Relay Model"},
                )
                catalog_with_own = build(
                    with_own_models, forward_api, "2026-11-25", history
                )
                self.assertEqual(
                    catalog_with_own["aliases"],
                    [
                        {
                            "id": "relay/retargeted",
                            "canonical_id": "publisher/current",
                        },
                        {"id": "retired/model", "canonical_id": "publisher/old"},
                    ],
                )
                self.assertIn(
                    {"id": "relay/model", "name": "Relay Model"},
                    catalog_with_own["models"],
                )

    def test_present_entry_check_uses_rust_full_id_equivalence(self):
        history = {
            "aliases": [
                {"id": "relay-one/model-v1", "canonical_id": "publisher/old"},
                {"id": "relay-one/model-v1:FREE", "canonical_id": "publisher/old"},
                {"id": "relay-one/retired-v2", "canonical_id": "publisher/old"},
            ]
        }
        base_models = models(
            {"id": "publisher/old", "name": "Old Model"},
            {"id": "publisher/current", "name": "Current Model"},
            {"id": "relay-one/model-v1", "name": "Relay Model V1"},
        )
        # 仅大小写或 -/./_ 分隔符不同时，仍属于同一当前条目，不得误判为退休。
        variants = [
            ("missing", {}),
            ("exact_self", {"canonical_model_id": "Relay_One/Model.V1"}),
            ("equivalent_self", {"canonical_model_id": "relay-one/model-v1"}),
            ("unlisted", {"canonical_model_id": "publisher/new"}),
        ]
        for label, entry_body in variants:
            with self.subTest(case=label):
                forward_api = api(
                    Relay_One={
                        "Model.V1": entry_body,
                        "Model_V1:free": {},
                    },
                    other_relay={"retired-v2": {}},
                )
                catalog = build(base_models, forward_api, "2026-11-25", history)
                # other_relay/retired-v2 命名空间不同，不影响 relay-one/retired-v2 恢复。
                self.assertEqual(
                    catalog["aliases"],
                    [
                        {
                            "id": "relay-one/retired-v2",
                            "canonical_id": "publisher/old",
                        }
                    ],
                )
                self.assertEqual(catalog["source"]["history"]["aliases"], 1)
                reversed_api = {
                    provider: {
                        "models": dict(reversed(list(body["models"].items())))
                    }
                    for provider, body in reversed(list(forward_api.items()))
                }
                self.assertEqual(
                    build(
                        dict(reversed(list(base_models.items()))),
                        reversed_api,
                        "2026-11-25",
                        {"aliases": list(reversed(history["aliases"]))},
                    ),
                    catalog,
                )

        # 大小写/分隔符不同的现行条目提供有效新目标时，只保留新目标，不残留旧别名。
        retargeted = build(
            base_models,
            api(
                Relay_One={
                    "Model.V1": {"canonical_model_id": "publisher/current"},
                    "Model_V1:free": {},
                }
            ),
            "2026-11-25",
            history,
        )
        self.assertEqual(
            retargeted["aliases"],
            [
                {
                    "id": "Relay_One/Model.V1",
                    "canonical_id": "publisher/current",
                },
                {"id": "relay-one/retired-v2", "canonical_id": "publisher/old"},
            ],
        )
        self.assertEqual(retargeted["source"]["history"]["aliases"], 1)

        # `:` 变体结构保留语义：仅有 `:free` 当前条目时，无变体的基础历史条目仍视作已退休。
        variant_only = build(
            base_models,
            api(Relay_One={"Model_V1:free": {}}),
            "2026-11-25",
            {"aliases": [{"id": "relay-one/model-v1", "canonical_id": "publisher/old"}]},
        )
        self.assertEqual(
            variant_only["aliases"],
            [{"id": "relay-one/model-v1", "canonical_id": "publisher/old"}],
        )

        # 规范模型层同样按完整 ID 等价判定：大小写/分隔符不同时不重复写入历史模型。
        model_eq = build(
            models({"id": "Publisher/Old_V1", "name": "Current Name"}),
            api(),
            "2026-11-25",
            {"models": [{"id": "publisher/old-v1", "name": "Historical Name"}]},
        )
        self.assertEqual(
            model_eq["models"],
            [{"id": "Publisher/Old_V1", "name": "Current Name"}],
        )
        self.assertEqual(model_eq["source"]["history"]["models"], 0)

    def test_weak_evidence_history_alias_is_archived_and_not_emitted(self):
        history = {
            "aliases": [
                {
                    "id": "fireworks-ai/accounts/fireworks/routers/kimi-k3-spot",
                    "canonical_id": "moonshotai/kimi-k3",
                    "evidence_strength": "upstream_assumed",
                },
                {
                    "id": "retired/verified-model",
                    "canonical_id": "moonshotai/kimi-k3",
                },
            ]
        }
        base_models = models(
            {"id": "moonshotai/kimi-k3", "name": "Kimi K3"},
            {"id": "moonshotai/kimi-k3.1-pro", "name": "Kimi K3.1 Pro"},
        )
        # 现行目录缺失时，弱证据历史条目不写入可用别名，可信历史条目照常补回。
        offline_catalog = build(base_models, api(), "2026-11-25", history)
        self.assertEqual(
            offline_catalog["aliases"],
            [
                {
                    "id": "retired/verified-model",
                    "canonical_id": "moonshotai/kimi-k3",
                }
            ],
        )
        self.assertEqual(offline_catalog["source"]["history"]["aliases"], 1)

        # 现行目录日后提供独立、有效映射时，按现行规则输出，不受历史弱证据标记阻断。
        for target in ("moonshotai/kimi-k3", "moonshotai/kimi-k3.1-pro"):
            with self.subTest(current_target=target):
                current_api = {
                    "fireworks-ai": {
                        "models": {
                            "accounts/fireworks/routers/kimi-k3-spot": {
                                "canonical_model_id": target
                            }
                        }
                    }
                }
                live_catalog = build(base_models, current_api, "2026-11-25", history)
                self.assertEqual(
                    live_catalog["aliases"],
                    [
                        {
                            "id": "fireworks-ai/accounts/fireworks/routers/kimi-k3-spot",
                            "canonical_id": target,
                        },
                        {
                            "id": "retired/verified-model",
                            "canonical_id": "moonshotai/kimi-k3",
                        },
                    ],
                )
                self.assertEqual(live_catalog["source"]["history"]["aliases"], 1)

        # 未识别的 evidence_strength 必须显式报错，防止拼写错误悄然放行弱证据。
        with self.assertRaises(ValueError):
            build(
                base_models,
                api(),
                "2026-11-25",
                {
                    "aliases": [
                        {
                            "id": "retired/bad-strength",
                            "canonical_id": "moonshotai/kimi-k3",
                            "evidence_strength": "assumed",
                        }
                    ]
                },
            )

    def test_evidenced_history_fixture_covers_retired_aliases_and_v2_only_models(self):
        import json
        import pathlib

        root = pathlib.Path(__file__).resolve().parents[1]
        history = json.loads(
            (root / "scripts/fixtures/model_display_catalog_history.json").read_text(
                encoding="utf-8"
            )
        )
        current = {
            entry["canonical_id"]: {
                "id": entry["canonical_id"],
                "name": "Current upstream name",
            }
            for entry in history["aliases"]
        }
        catalog = build(current, api(), "2026-11-25", history)
        self.assertEqual(len(catalog["models"]), len(current) + 4)
        self.assertEqual(len(history["aliases"]), 26)
        self.assertEqual(len(catalog["aliases"]), 25)
        self.assertEqual(catalog["source"]["history"]["models"], 4)
        self.assertEqual(catalog["source"]["history"]["aliases"], 25)
        for entry in history["aliases"]:
            self.assertTrue(entry["reason"])
            self.assertTrue(entry["evidence"].startswith("https://github.com/"))
        weak = [
            entry
            for entry in history["aliases"]
            if entry.get("evidence_strength") == "upstream_assumed"
        ]
        self.assertEqual(
            [entry["id"] for entry in weak],
            ["fireworks-ai/accounts/fireworks/routers/kimi-k3-spot"],
        )
        self.assertEqual(
            weak[0]["evidence"],
            "https://github.com/anomalyco/models.dev/commit/45aaa380ff10",
        )
        emitted_ids = {entry["id"] for entry in catalog["aliases"]}
        self.assertNotIn(
            "fireworks-ai/accounts/fireworks/routers/kimi-k3-spot",
            emitted_ids,
        )

    def test_identity_key_matches_rust_model_identity_rules(self):
        # 与 crates/tokenscope-core/src/model_identity.rs 的规则逐项对齐。
        for raw in [
            "claude-opus-5-5",
            "claude-opus-5.5",
            "Claude_Opus_5_5",
            "CLAUDE.OPUS.5.5",
            "claudeopus55",
            "  Claude-Opus_5.5  ",
        ]:
            self.assertEqual(identity_key(raw), "claudeopus55", raw)
        self.assertEqual(
            identity_key("nano-gpt/qwen/qwen3.8-27b:thinking"),
            "nanogpt/qwen/qwen3827b:thinking",
        )
        self.assertEqual(identity_key("Relay/Opus55:FREE"), "relay/opus55:free")
        self.assertNotEqual(
            identity_key("Relay/Opus55:FREE"),
            identity_key("Relay/Opus55:thinking"),
        )
        # 显式空变体保留 `:`；非 ASCII 字符只折叠 ASCII 大小写。
        self.assertEqual(identity_key("relay/opus55:"), "relay/opus55:")
        self.assertNotEqual(identity_key("relay/opus55:"), identity_key("relay/opus55"))
        self.assertEqual(identity_key("Relay/模型-Δ.1"), "relay/模型Δ1")
        # 基名压缩后为空时返回 None。
        for empty in ["", "   ", "-._", "---", "provider/---", "provider/-._:free"]:
            self.assertIsNone(identity_key(empty), empty)

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