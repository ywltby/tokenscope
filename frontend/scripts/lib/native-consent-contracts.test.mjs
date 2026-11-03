// @vitest-environment node
// 同意终态断言契约：直接执行 native-acceptance.mjs 里的
// strictConsentState（单一事实源），用真实反例揭示正则误放行。
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "vitest";
import { parse as parseToml } from "smol-toml";
import { parseTOML } from "toml-eslint-parser";

const source = fs.readFileSync(new URL("../native-acceptance.mjs", import.meta.url), "utf8");

function loadStrictConsentState() {
  const start = source.indexOf("function strictConsentState");
  assert.ok(start >= 0, "strictConsentState 未找到");
  let depth = 0;
  let end = -1;
  for (let i = source.indexOf("{", start); i < source.length; i += 1) {
    const ch = source[i];
    if (ch === "{") depth += 1;
    else if (ch === "}") {
      depth -= 1;
      if (depth === 0) {
        end = i + 1;
        break;
      }
    }
  }
  assert.ok(end > start, "strictConsentState 边界未找到");
  return new Function(
    "parseToml",
    "parseTOML",
    `return ${source.slice(start, end)};`,
  )(parseToml, parseTOML);
}

const strictConsentState = loadStrictConsentState();
const REV = 3;

function state(text) {
  return strictConsentState(text, REV);
}

test("当前版本四项精确匹配通过", () => {
  const text =
    "privacy_policy_accepted = true\n" +
    `privacy_policy_revision = ${REV}\n` +
    "user_agreement_accepted = true\n" +
    `user_agreement_revision = ${REV}\n`;
  assert.equal(state(text).ok, true, state(text).detail);
});

test("注释中的期望字段不能掩盖实际 false/30", () => {
  const text =
    "# privacy_policy_accepted = true\n" +
    "privacy_policy_accepted = false\n" +
    "# privacy_policy_revision = 3\n" +
    "privacy_policy_revision = 30\n" +
    "# user_agreement_accepted = true\n" +
    "user_agreement_accepted = false\n" +
    "# user_agreement_revision = 3\n" +
    "user_agreement_revision = 30\n";
  assert.equal(state(text).ok, false, state(text).detail);
});

test("子表与点号键不算顶层同意记录", () => {
  const subtable =
    "[notes]\n" +
    "privacy_policy_accepted = true\n" +
    `privacy_policy_revision = ${REV}\n` +
    "user_agreement_accepted = true\n" +
    `user_agreement_revision = ${REV}\n`;
  assert.equal(state(subtable).ok, false, state(subtable).detail);
  const dotted =
    `consent.privacy_policy_accepted = true\n` +
    `consent.privacy_policy_revision = ${REV}\n` +
    `consent.user_agreement_accepted = true\n` +
    `consent.user_agreement_revision = ${REV}\n`;
  assert.equal(state(dotted).ok, false, state(dotted).detail);
});

test("30 前缀、浮点与缺协议字段都被拒绝", () => {
  const prefix =
    "privacy_policy_accepted = true\n" +
    "privacy_policy_revision = 30\n" +
    "user_agreement_accepted = true\n" +
    "user_agreement_revision = 30\n";
  assert.equal(state(prefix).ok, false, state(prefix).detail);
  const float =
    "privacy_policy_accepted = true\n" +
    "privacy_policy_revision = 3.0\n" +
    "user_agreement_accepted = true\n" +
    "user_agreement_revision = 3.0\n";
  assert.equal(state(float).ok, false, state(float).detail);
  const missingAgreement =
    "privacy_policy_accepted = true\n" + `privacy_policy_revision = ${REV}\n`;
  assert.equal(state(missingAgreement).ok, false, state(missingAgreement).detail);
});

test("两份记录修订不一致被拒绝", () => {
  const text =
    "privacy_policy_accepted = true\n" +
    `privacy_policy_revision = ${REV}\n` +
    "user_agreement_accepted = true\n" +
    `user_agreement_revision = ${REV + 1}\n`;
  assert.equal(state(text).ok, false, state(text).detail);
});

test("非法 UTF-8 字节被拒绝", () => {
  const bytes = Buffer.from(
    "privacy_policy_accepted = true\n" +
      `privacy_policy_revision = ${REV}\n` +
      "user_agreement_accepted = true\n" +
      `user_agreement_revision = ${REV}\n`,
    "utf8",
  );
  // 在协议键值后注入孤立的 0xFF（String::from_utf8 会 InvalidData）。
  const broken = Buffer.concat([bytes, Buffer.from([0xff])]);
  assert.equal(state(broken).ok, false);
});
