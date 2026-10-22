// @vitest-environment node
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "vitest";

test("local price endpoints count hits and support partial failure and disconnection", async () => {
  const { startAcceptanceIo } = await import("./acceptance-io.mjs");
  const io = await startAcceptanceIo();
  try {
    const models = await fetch(`${io.base}/modelsdev/api.json`);
    assert.equal(models.status, 200);
    assert.ok((await models.json()).anthropic.models["claude-opus-5.5"]);
    io.setMode("openrouter", "500");
    assert.equal((await fetch(`${io.base}/openrouter/api/v1/models`)).status, 500);
    io.setMode("openrouter", "ok");
    assert.equal((await (await fetch(`${io.base}/openrouter/api/v1/models`)).json()).data.length, 2);
    io.setMode("modelsdev", "down");
    await assert.rejects(fetch(`${io.base}/modelsdev/api.json`));
    assert.equal(io.snapshot().hits.modelsdev.length, 2);
    assert.equal(io.snapshot().hits.openrouter.length, 2);
    assert.deepEqual(io.snapshot().proxy_attempts, []);
    assert.equal(io.env.NO_PROXY, "127.0.0.1,localhost");
    assert.equal(io.env.TOKENSCOPE_ACCEPTANCE_OPENER, "record");
    assert.match(io.env.HTTP_PROXY, /^http:\/\/127\.0\.0\.1:\d+$/);
    for (const key of ["HTTPS_PROXY","http_proxy","https_proxy","ALL_PROXY","all_proxy"]) {
      assert.equal(io.env[key],io.env.HTTP_PROXY,`${key} 必须覆盖宿主值`);
    }
  } finally { await io.close(); }
});

test("path events must carry a path and successful business scenarios must have trace evidence", async () => {
  const { validateAcceptanceTrace, missingBusinessEvents } = await import("./acceptance-io.mjs");
  const root = fs.mkdtempSync(path.join(os.tmpdir(),"tokenscope-ac02-required-path-"));
  try {
    for (const event of ["history_open","source_root_read","ccs_open","pricing_fetch","opener","settings_write","consent_saved"]) {
      const row = {seq:1,monotonic_ms:0,utc:"2026-10-22T00:00:00Z",event};
      assert.ok(validateAcceptanceTrace(root,JSON.stringify(row)+"\n").violations.length > 0,`${event} 缺少路径必须失败`);
    }
    assert.deepEqual(missingBusinessEvents([]),["business_ready","collect_round_start","history_open","source_root_read"]);
  } finally { fs.rmSync(root,{recursive:true}); }
});

test("isolation cleanup enters the existing stop before cancellation and awaits both operations", async () => {
  const { beginIsolationCleanup } = await import("./acceptance-io.mjs");
  const order = [];
  let canceled = false;
  let finishKill;
  let finishStop;
  const cleanup = beginIsolationCleanup({
    terminate:() => { order.push("terminate"); return new Promise(resolve => {finishKill=resolve;}); },
    stop:() => { assert.equal(canceled,false,"既有 stop 取消检查必须保持成立"); order.push("stop"); return new Promise(resolve => {finishStop=resolve;}); },
    cancel:() => { canceled=true; order.push("cancel"); },
  });
  assert.deepEqual(order,["terminate","stop","cancel"]);
  let settled = false;
  cleanup.then(() => {settled=true;});
  finishKill({pids:[123]});
  await Promise.resolve();
  assert.equal(settled,false,"不能在退出确认前把隔离收尾当作完成");
  finishStop({action:"stopped"});
  assert.equal((await cleanup).action,"stopped");
});

test("isolation cleanup still awaits exit confirmation when immediate termination fails", async () => {
  const { beginIsolationCleanup, proxyEnvironment } = await import("./acceptance-io.mjs");
  const failureEnv = proxyEnvironment("http://127.0.0.1:9","");
  for (const name of ["HTTP_PROXY","HTTPS_PROXY","http_proxy","https_proxy","ALL_PROXY","all_proxy"]) {
    assert.equal(failureEnv[name],"http://127.0.0.1:9");
  }
  assert.equal(failureEnv.NO_PROXY,"");
  assert.equal(failureEnv.no_proxy,"");
  let finishStop;
  let settled = false;
  const result = beginIsolationCleanup({
    terminate:() => Promise.reject(new Error("synthetic termination failure")),
    stop:() => new Promise(resolve => {finishStop=resolve;}),
    cancel:() => {},
  }).catch(error => {settled=true; return error;});
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(settled,false);
  finishStop({action:"confirmed"});
  assert.ok(await result instanceof AggregateError);
});

test("isolation termination freezes descendants before killing the host", async () => {
  const { terminateAfterSnapshot } = await import("./acceptance-io.mjs");
  const descendants = [];
  let finishSnapshot;
  let killed = false;
  const ending = terminateAfterSnapshot({
    refresh:() => new Promise(resolve => {finishSnapshot=resolve;}),
    terminate:() => { assert.deepEqual(descendants,[123]); killed=true; return {pids:[123]}; },
  });
  assert.equal(killed,false);
  descendants.push(123);
  finishSnapshot({ok:true});
  assert.deepEqual((await ending).pids,[123]);
  await assert.rejects(terminateAfterSnapshot({
    refresh:() => ({ok:false,error:"synthetic probe failure"}),
    terminate:() => ({pids:[]}),
  }),/后代身份/);
});

test("trace validation checks actual ordering and isolated paths, including partial appends", async () => {
  const { validateAcceptanceTrace, settingsFingerprint } = await import("./acceptance-io.mjs");
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tokenscope-ac02-trace-validator-"));
  try {
    const records = [{seq:1, monotonic_ms:0, utc:"2026-10-22T00:00:00Z", event:"history_open", path:path.join(root,"tokenscope","history.db")}];
    const text = records.map(row => JSON.stringify(row)).join("\n") + "\n";
    assert.deepEqual(validateAcceptanceTrace(root,text).records, records);
    assert.deepEqual(validateAcceptanceTrace(root,text).violations, []);
    assert.deepEqual(validateAcceptanceTrace(root,text + '{"seq":2', {partial:true}).violations, []);
    assert.equal(validateAcceptanceTrace(root,text + '{"seq":2').violations.length, 1);
    assert.equal(validateAcceptanceTrace(root,JSON.stringify({...records[0],seq:2})+"\n").violations.length, 1);
    assert.deepEqual(settingsFingerprint(Buffer.from("hello")), {algorithm:"fnv1a64",value:"a430d84680aabd0b"});
  } finally { fs.rmSync(root,{recursive:true}); }
});

test("privacy file comparison includes every data file except the consent settings", async () => {
  const { snapshotBusinessFiles, changedBusinessFiles } = await import("./acceptance-io.mjs");
  const root = fs.mkdtempSync(path.join(os.tmpdir(),"tokenscope-ac02-business-files-"));
  try {
    fs.writeFileSync(path.join(root,"settings.toml"),"consent=false");
    fs.writeFileSync(path.join(root,"history.db"),"seed");
    const before = snapshotBusinessFiles(root);
    fs.writeFileSync(path.join(root,"settings.toml"),"consent=true");
    assert.deepEqual(changedBusinessFiles(before,snapshotBusinessFiles(root)),[]);
    for (const name of ["history.db-wal","history.db-shm","previously-unknown-business.bin"]) fs.writeFileSync(path.join(root,name),"new");
    fs.writeFileSync(path.join(root,"history.db"),"edit");
    assert.deepEqual(changedBusinessFiles(before,snapshotBusinessFiles(root)),
      ["history.db","history.db-shm","history.db-wal","previously-unknown-business.bin"]);
  } finally { fs.rmSync(root,{recursive:true}); }
});
