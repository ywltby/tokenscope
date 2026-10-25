// AC03：统一合成来源经真实 WebView2 / Rust 采集、查询与设置事务验收。
import fs from "node:fs";
import path from "node:path";
import { createUnifiedContext, nextQueryDimension } from "../lib/native-unified.mjs";
import { judgeIdentity } from "../lib/native-identity-checks.mjs";
import { readAcceptanceTrace } from "../lib/acceptance-io.mjs";

const labels = { day: "按时间", model: "按模型", project: "按项目", agent: "按应用" };

export async function nativeIdentity(context) {
  const { inst, ck, ev } = context;
  const ui = createUnifiedContext(context);
  ev.identity = {};
  const check = (name, evidence) => {
    ev.identity[name] = evidence;
    const result = judgeIdentity(name, evidence);
    ck.add(name, result.ok, result.basis);
  };
  const dimension = async (by) => {
    await ui.click(
      ui
        .page()
        .locator('[aria-label="聚合维度"]')
        .getByRole("radio", { name: labels[by], exact: true }),
    );
    try {
      await ui.waitUntil(
        async () => {
          const snapshot = await ui.querySnapshot();
          return snapshot.sameQuery && snapshot.domMatches && snapshot.summary.by === by;
        },
        20000,
        `等待${labels[by]}的真实查询`,
      );
    } catch (error) {
      // WB07 取证：等待失败时把最后一次快照写入证据，区分探针对齐与 DOM 匹配。
      ev.identity ??= {};
      ev.identity.dimension_failure = {
        by,
        snapshot: await ui
          .querySnapshot()
          .catch((probeError) => ({ probeError: String(probeError) })),
      };
      throw error;
    }
    return ui.querySnapshot();
  };
  const queryAfterUiChange = async () => {
    // 重启恢复的首屏查询可能早于探针；以真实选中项决定下一次点击，不能重复点中项。
    const selected = await ui
      .page()
      .locator('[aria-label="聚合维度"] [role="radio"][aria-checked="true"]')
      .getAttribute("aria-label");
    await dimension(nextQueryDimension(selected));
    return ui.querySnapshot();
  };
  const summaryLabels = () =>
    ui
      .page()
      .locator(".usage-card .dim-cell")
      .allInnerTexts()
      .then((texts) =>
        texts.map((text) => text.replace(/\s*›\s*$/, "").trim()).filter((text) => text !== "合计"),
      );
  const detailText = () => ui.page().locator(".events-card").innerText();
  const drill = async (by, group) => {
    const before = (await ui.readProbe()).invokes.length;
    const matching = ui
      .page()
      .locator(".usage-card")
      .getByRole("button", { name: `查看 ${group.label ?? group.key} 的请求明细`, exact: true });
    // 同名项目的可访问名称相同；选中组在响应顺序中的位置，与真实表格顺序对应。
    const peers = (await ui.querySnapshot()).summary.groups.filter(
      (item) => item.key !== "合计" && (item.label ?? item.key) === (group.label ?? group.key),
    );
    const ordinal = peers.findIndex((item) => item.key === group.key);
    await ui.click(matching.nth(Math.max(0, ordinal)));
    await ui.waitUntil(
      async () => {
        const snapshot = await ui.querySnapshot();
        const calls = (await ui.readProbe()).invokes.slice(before);
        return (
          snapshot.sameQuery &&
          snapshot.domMatches &&
          snapshot.eventArgs?.[by] === group.key &&
          calls.some(
            (record) =>
              record.command === "query_events" &&
              record.status === "fulfilled" &&
              record.args?.[by] === group.key,
          )
        );
      },
      20000,
      `${by} 行下钻`,
    );
    return ui.querySnapshot();
  };
  const clearDrill = async () => {
    await ui.click(ui.page().locator(".events-card .filter-chip"));
    await ui.waitUntil(
      async () => {
        const snapshot = await ui.querySnapshot();
        return (
          snapshot.sameQuery &&
          snapshot.domMatches &&
          ["day", "model", "project"].every((key) => snapshot.eventArgs?.[key] == null) &&
          (await ui.page().locator(".events-card .filter-chip").count()) === 0
        );
      },
      20000,
      "清除下钻筛选",
    );
    return ui.querySnapshot();
  };

  await ui.installProbe();
  try {
    await inst.waitForData();
    await inst.setWindowSize(1180, 820);
    // 通过设置按钮同步受控回环价格桩，得到 models.dev 的友好展示名。
    await ui.nav("设置");
    await ui.section("价格");
    await ui.click(ui.page().getByRole("button", { name: "同步在线价格", exact: true }));
    await ui.waitUntil(
      async () => (await ui.last("sync_pricing_openrouter"))?.status === "fulfilled",
      20000,
      "合成价格同步",
    );
    await ui.waitUntil(
      () => fs.existsSync(path.join(inst.root, "tokenscope", "pricing-modelsdev.json")),
      20000,
      "合成主源快照落盘",
    );
    await ui.nav("汇总");
    const project = await dimension("project");
    if (project.summary.totals.requests !== 16 || project.events.total !== 16) {
      ev.identity_initial = project;
      throw new Error(
        "统一 fixture 初始请求应为 8 Claude + 2 Codex + 两版 WorkBuddy 各 3，不允许额外填充事件",
      );
    }
    const all = project;
    const visibleLabels = await summaryLabels();
    const shared = project.summary.groups.find(
      (group) => group.key === "C:/acceptance/workspace/tokenscope",
    );
    if (!shared) throw new Error("缺少共享项目行");
    const projectDetail = await drill("project", shared);
    check("project_same_path_merges_across_agents", {
      project,
      detail: projectDetail,
      detailText: await detailText(),
    });
    await clearDrill();
    check("project_subdir_stays_in_root", { all });
    check("project_out_of_root_switches", { all });
    check("project_label_is_last_segment", { project, visibleLabels });
    const model = await dimension("model");
    check("model_variants_merge_with_friendly_name", {
      model,
      visibleLabels: await summaryLabels(),
      usageText: await ui.root().innerText(),
    });
    const opus = model.summary.groups.find((group) => group.key === "claudeopus55");
    if (!opus) throw new Error("缺少归一化 Opus 模型行");
    const modelDetail = await drill("model", opus);
    check("model_detail_keeps_raw_names", { detail: modelDetail, detailText: await detailText() });
    await clearDrill();

    const drills = [];
    for (const by of ["day", "model", "project"]) {
      const snapshot = await dimension(by);
      const group = snapshot.summary.groups.find((item) => item.key !== "合计");
      if (!group) throw new Error(`缺少 ${by} 可下钻行`);
      const selected = await drill(by, group);
      const filtered = await ui.page().locator(".events-card .filter-chip").isVisible();
      const cleared = await clearDrill();
      drills.push({
        type: by,
        key: group.key,
        expectedRequests: group.requests,
        snapshot: selected,
        filtered,
        cleared,
        clearedFilter: (await ui.page().locator(".events-card .filter-chip").count()) === 0,
      });
    }
    check("drill_from_day_model_project_rows", { drills });

    // WB07：四应用来源筛选独立选中 + 子 agent 计入 + 失败调用不计。
    // 「全部」的 IPC 实参是字符串 "all"（parse_agent 接受 None | Some("all")）。
    const agentParamOf = { WorkBuddy: "workbuddy", "WorkBuddy AI": "workbuddy-ai", 全部: "all" };
    const clickAgent = async (name) => {
      await ui.click(
        ui.page().locator('[aria-label="数据来源"]').getByRole("radio", { name, exact: true }),
      );
      await ui.waitUntil(
        async () => {
          const snapshot = await ui.querySnapshot();
          // WB07 run2 教训：只等 begin 出现会在 summary/events 落定前返回；
          // 必须等同查询会话对齐且 begin 的 agent 参数与目标一致。
          return (
            snapshot.sameQuery &&
            snapshot.domMatches &&
            snapshot.beginCall?.args?.agent === agentParamOf[name]
          );
        },
        20000,
        `切换来源筛选：${name}`,
      );
      return ui.querySnapshot();
    };
    const wb = await clickAgent("WorkBuddy");
    const wbai = await clickAgent("WorkBuddy AI");
    check("workbuddy_sources_selected_independently", { wb, wbai, all: await clickAgent("全部") });
    check("workbuddy_subagent_usage_counted", { wb: await clickAgent("WorkBuddy") });
    check("workbuddy_failed_call_not_counted", { wb: await clickAgent("WorkBuddy") });
    // 回到全部，保持后续基线断言的数字口径。
    await clickAgent("全部");
    await ui.waitUntil(
      async () => (await ui.querySnapshot()).summary?.totals.requests === 16,
      20000,
      "回到全部来源",
    );

    // 先保存 Claude 历史基线，再改变来源；停用不能把历史从应用筛选中抹掉。
    await ui.click(
      ui
        .page()
        .locator('[aria-label="数据来源"]')
        .getByRole("radio", { name: "Claude Code", exact: true }),
    );
    await ui.waitUntil(async () => {
      const snapshot = await ui.querySnapshot();
      return snapshot.sameQuery && snapshot.domMatches && snapshot.summary.totals.requests === 8;
    });
    const beforeClaude = (await ui.querySnapshot()).summary.totals;
    await ui.click(
      ui
        .page()
        .locator('[aria-label="数据来源"]')
        .getByRole("radio", { name: "全部", exact: true }),
    );
    await ui.waitUntil(async () => (await ui.querySnapshot()).summary?.totals.requests === 16);
    const before = await ui.querySnapshot();

    await ui.nav("设置");
    await ui.section("数据源");
    await ui.select('[aria-label="自动读取间隔"]', "5 秒");
    await ui.waitUntil(() => ui.savedSetting("collect_interval_secs", "5"));
    await ui.nav("汇总");
    const files = [
      path.join(
        inst.root,
        "sources",
        "claude",
        "projects",
        "-acceptance-workspace-tokenscope",
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl",
      ),
    ];
    if (!fs.existsSync(files[0])) throw new Error("待删除的主 Claude 合成日志不存在");
    fs.unlinkSync(files[0]);
    const fileAbsent = !fs.existsSync(files[0]);
    if (!fileAbsent) throw new Error("主 Claude 合成日志删除未生效");
    const firstProcess = inst.launches.length === 1;
    if (!firstProcess) throw new Error("后端轮次序号关联要求尚未重启的首进程");
    const traceAfterDelete = readAcceptanceTrace(inst.root, { partial: true });
    if (traceAfterDelete.violations.length)
      throw new Error(traceAfterDelete.violations.join(" | "));
    // 同步追踪在实际领队读取之前写入并flush。以删除后的后端seq为界，
    // 页面消息即使延迟送达，也不能把删除前旧轮次当成“下一轮”。
    const traceBoundarySeq = traceAfterDelete.records.at(-1)?.seq ?? 0;
    const deletedAtMs = await ui.page().evaluate(() => performance.now());
    let round;
    let backendTrace;
    await ui.waitUntil(
      async () => {
        const trace = readAcceptanceTrace(inst.root, { partial: true });
        if (trace.violations.length) throw new Error(trace.violations.join(" | "));
        const starts = trace.records.filter((record) => record.event === "collect_round_start");
        const next = starts.find((record) => record.seq > traceBoundarySeq);
        if (!next) return false;
        // report.rs持串行锁逐次 begin_run -> before_leader_work，因此首进程中
        // 第N条collect_round_start正是runId=N；删除前尚未发生任何重启。
        const runId = starts.indexOf(next) + 1;
        const callbacks = (await ui.readProbe()).callbacks;
        const start = callbacks.find(
          (record) =>
            record.kind === "status-enter" && record.phase === "running" && record.runId === runId,
        );
        const end =
          start &&
          callbacks.find(
            (record) =>
              record.kind === "status-exit" &&
              record.runId === start.runId &&
              record.phase === "succeeded",
          );
        if (end) {
          round = { start, end };
          backendTrace = trace.records;
        }
        return Boolean(round);
      },
      20000,
      "删除合成日志后真实下一轮采集完成",
    );
    const queriedAtMs = await ui.page().evaluate(() => performance.now());
    const after = await queryAfterUiChange();
    const afterQueryCall = (await ui.readProbe()).invokes.findLast(
      (record) =>
        record.command === "query_begin" &&
        record.status === "fulfilled" &&
        record.value?.queryId === after.summaryQuery,
    );
    check("source_file_deletion_keeps_history", {
      before,
      after,
      deletedFiles: files,
      fileAbsent,
      deletedAtMs,
      firstProcess,
      traceBoundarySeq,
      backendTrace,
      round,
      queriedAtMs,
      afterQueryCall,
    });

    await ui.nav("设置");
    await ui.section("用量数据");
    await ui.click(ui.page().getByRole("button", { name: "重扫日志", exact: true }));
    await ui.waitUntil(async () => (await ui.last("refresh_cache"))?.status === "fulfilled");
    const rescanCall = await ui.last("refresh_cache");
    await ui.nav("汇总");
    const rescanned = await queryAfterUiChange();
    await ui.saveProbe("before-index-rebuild");
    await ui.closeForRestart();
    const cleanExit = inst.proc.exitCode === 0;
    const index = path.join(inst.root, "tokenscope", "pricing-index.json");
    const indexBefore = fs.existsSync(index);
    if (!indexBefore) throw new Error("应存在的派生价格索引未生成，不可冒充已删除");
    fs.unlinkSync(index);
    const indexDeleted = !fs.existsSync(index);
    await inst.launch();
    await ui.installProbe();
    await inst.waitForData();
    const rebuilt = await queryAfterUiChange();
    check("rescan_and_rebuild_keep_history", {
      before,
      rescanned,
      rebuilt,
      rescanCall,
      cleanExit,
      indexDeleted,
      indexRecreated: fs.existsSync(index),
    });

    await ui.nav("设置");
    await ui.section("数据源");
    const sourceSwitch = ui.page().getByRole("switch", { name: "Claude Code 启用", exact: true });
    await ui.click(sourceSwitch);
    await ui.waitUntil(async () => {
      const call = await ui.last("source_config_set");
      return (
        call?.status === "fulfilled" &&
        call.args.agent === "claude" &&
        call.args.enabled === false &&
        (await sourceSwitch.getAttribute("aria-checked")) === "false"
      );
    });
    const disableCall = await ui.last("source_config_set");
    const disabledSwitch = (await sourceSwitch.getAttribute("aria-checked")) === "false";
    // 总开关提交不会主动重读列表；真实切页重新激活设置页后再观察后端状态。
    await ui.nav("汇总");
    await ui.nav("设置");
    await ui.section("数据源");
    await ui.waitUntil(async () =>
      (await ui.last("source_paths_list"))?.value?.some(
        (row) => row.agent === "claude" && row.enabled === false,
      ),
    );
    const sourceRows = (await ui.last("source_paths_list")).value;
    await ui.nav("汇总");
    await ui.click(
      ui
        .page()
        .locator('[aria-label="数据来源"]')
        .getByRole("radio", { name: "Claude Code", exact: true }),
    );
    await ui.waitUntil(async () => {
      const snapshot = await ui.querySnapshot();
      return (
        snapshot.sameQuery && snapshot.domMatches && snapshot.beginCall?.args?.agent === "claude"
      );
    });
    const claude = await ui.querySnapshot();
    check("source_disable_keeps_history", {
      beforeClaude,
      claude,
      disableCall,
      sourceRows,
      disabledSwitch,
      beginArgs: claude.beginCall.args,
    });
    await ui
      .page()
      .screenshot({ path: path.join(inst.opts.shots, "ui-identity-history-retained.png") });
  } finally {
    try {
      await ui.saveProbe();
    } catch (error) {
      ev.identity_probe_error = String(error);
    }
    try {
      await ui.cleanup();
    } catch (error) {
      ev.identity_cleanup_error = String(error);
    }
  }
}
