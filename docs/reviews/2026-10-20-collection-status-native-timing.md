# 原生读取状态验收：查询发起时序取证修正

## 失败与证据边界

PR #18 的 [Native UI E2E 39363530556](https://github.com/ywltby/tokenscope/actions/runs/39363530556) 中，读取状态场景 20 项有 1 项失败：`query_publishes_within_reading_window`。

工件的读取样本始于 20892ms；21851ms 仍为读取态，21914ms 已显示新增请求及新查询身份，最后两个样本相隔 63ms。旧取样对各 DOM 属性分别执行异步 CDP 调用，甚至出现 revision 仍为上一轮、runId 已属下一轮的组合。它不能证明准确的采集完成或查询开始时刻，也不能据此认定产品在等待动画。

旧判定还有独立的逻辑缺口：无论采集实际耗时多长，都要求查询开始时仍处于读取显示期。产品的 1 秒是最短反馈时间，不是采集性能上限；慢轮次完成时可以立即结束反馈，同时发起查询。用轮询中的代次或“刷新中”文案代替真实调用，会误判，也可能漏检。

## 修复约束与实现

- DOM 属性改为同一个 `page.evaluate` 内同步读取，附页面 `performance.now()`；不再拼接跨时刻样本。
- 仅在隔离原生场景中观察 Tauri 的调试用 `callbacks` Map。原回调的参数、返回值、异常和一次性移除语义均保留；不修改产品事件、定时器或查询调度。
- 查询时刻取成功 `query_begin` 响应所对应回调的注册时刻，响应提供实际 `queryId` 和 `generation`。Tauri 的 invoke 在同一同步路径注册回调并发送 IPC，因此不会把后端查询耗时误当成前端延迟。
- 在当前可见、未翻页、无在途批次的场景中，要求最终落地的同一查询在本轮 running 之后、succeeded 回调处理结束之前已发起。采集快慢均采用同一规则。缺少起点、终态、响应身份或落地证据，一律失败。
- 检查名称和场景检查数量保留。记录 `callback_trace`，即使等待落地失败也尝试保存，并恢复被包装的回调。
- Tauri 调试接口不兼容时明确失败；不降级回轮询推断。接口依赖只存在于验收脚本。

## 回归验证

两组定向测试共 14 项：50ms / 1000ms / 2500ms 采集的即时查询；快轮次等动画、慢轮次额外等动画的反例；缺证据、错误查询身份、错误代次、无落地；回调转发、移除、恢复与抛错行为。

改实现前，判定测试 4 项失败、探针测试 2 项失败；实现后 14 项通过。日志分别保存于 `qa-artifacts/collection-publish-red.log`、`qa-artifacts/collection-probe-red.log`。这不是旧失败作业的重跑豁免；最终仍须由新 HEAD 的完整门禁与真实原生场景取证。

当前源码按 CI 相同的 `acceptance,tauri/custom-protocol` 参数构建后，隔离原生场景 20/20 通过，真实数据指纹不变。页面时钟记录 running=21989.30ms、succeeded 入口=21996.30ms、查询发起=21996.40ms、succeeded 处理结束=21997.20ms，查询响应与落地身份一致。工件在 `qa-artifacts/collection-status-direct-probe-local/ui-collection-status/evidence.json`。前端全量 615 项通过。

本地首次尝试使用了遗留旧二进制；重新构建后仍无法连接，启动证据确认端口属于新实例子进程，但 CDP HTTP 请求收到代理的 502。仅对验收子进程设置回环地址 `NO_PROXY` 后连接成功；未改变系统代理。两次连接失败工件分别保留在 `qa-artifacts/collection-status-direct-probe/` 与 `qa-artifacts/collection-status-direct-probe-current/`，不算场景通过证据。
