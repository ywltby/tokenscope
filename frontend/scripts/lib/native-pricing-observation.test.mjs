// @vitest-environment node
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, it, vi } from "vitest";
import { pricingVerdict } from "./native-pricing-checks.mjs";

const source = fs.readFileSync(new URL("../contracts/native-pricing.mjs", import.meta.url), "utf8");
const expressions = [...source.matchAll(/\bexpectedPath: ([^,\n]+),/g)].map(match => match[1]);
// 执行原生场景实际传入判定的属性表达式，不复制一份待测取证实现。
function expectedPath(file, filesystem) {
  expect(expressions).toHaveLength(1);
  return new Function("file", "fs", `return (${expressions[0]});`)(file, filesystem);
}
const shortPath = "C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\isolated\\tokenscope\\pricing.toml";
const longPath = "C:\\Users\\runneradmin\\AppData\\Local\\Temp\\isolated\\tokenscope\\pricing.toml";
const observation = expected => ({
  existedBefore: false, created: true, content: "[[model]]", expectedPath: expected,
  returnedPath: longPath, openerPaths: [longPath], recorded: true,
  processProbeOk: true, foreignProcesses: [],
});

it("实际期望路径先展开 Windows 短名，与后端和追踪指向同一文件", () => {
  const realpath = vi.fn(() => longPath);
  const expected = expectedPath(shortPath, { existsSync: () => true, realpathSync: { native: realpath } });
  expect(expected).toBe(longPath);
  expect(realpath).toHaveBeenCalledExactlyOnceWith(shortPath);
  expect(pricingVerdict("open_pricing_file_creates_template_in_root", observation(expected)).ok).toBe(true);
});

it("缺少实际文件时不能借原路径字符串通过", () => {
  const realpath = vi.fn();
  const expected = expectedPath(shortPath, { existsSync: () => false, realpathSync: { native: realpath } });
  expect(expected).toBe(null);
  expect(realpath).not.toHaveBeenCalled();
  expect(pricingVerdict("open_pricing_file_creates_template_in_root", observation(expected)).ok).toBe(false);
});

it("规范化失败必须保留错误，不能退回未经验证的原路径", () => {
  expect(() => expectedPath(shortPath, { existsSync: () => true, realpathSync: {
    native: () => { throw new Error("canonicalization failed"); },
  } })).toThrow("canonicalization failed");
});

it("路径规范化后，返回错误文件或 opener 记录错误文件仍然失败", () => {
  const expected = expectedPath(shortPath, { existsSync: () => true, realpathSync: { native: () => longPath } });
  const wrongReturn = observation(expected);
  wrongReturn.returnedPath = longPath.replace("pricing.toml", "other.toml");
  expect(pricingVerdict("open_pricing_file_creates_template_in_root", wrongReturn).ok).toBe(false);
  const wrongOpener = observation(expected);
  wrongOpener.openerPaths = [longPath.replace("isolated", "outside")];
  expect(pricingVerdict("open_pricing_file_creates_template_in_root", wrongOpener).ok).toBe(false);
});

it("真实临时文件的期望路径由文件系统解析，原路径单独留证", () => {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), "tokenscope-pricing-path-"));
  const file = path.join(folder, "pricing.toml");
  try {
    fs.writeFileSync(file, "[[model]]\n", "utf8");
    expect(expectedPath(file, fs)).toBe(fs.realpathSync.native(file));
    expect(source).toMatch(/\brawExpectedPath: file,/);
  } finally {
    fs.unlinkSync(file);
    fs.rmdirSync(folder);
  }
});
