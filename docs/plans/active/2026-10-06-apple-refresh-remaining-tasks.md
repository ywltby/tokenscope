# 苹果风格视觉翻新剩余任务（Task 3-7）

> **前置条件**：Task 1-2 已完成（token 适配 + 应用壳）。
> **执行方式**：每个 Task 独立 commit，按顺序执行，测试全绿后再进下一个。

## Task 3：汇总页头部与筛选栏

### 目标

- 大标题行：标题 + 时区/日期副标题，右侧状态胶囊（已更新 / 刷新中 / 缓存数据）+ 刷新按钮
- 筛选栏一行：来源（图标 + 文字分段）→ 维度分段 → 日期 → 时区，统一 32px
- 来源异常、错误重试改为内联通知条，放在筛选栏下方

### 文件清单

- **修改** `frontend/src/views/Dashboard.vue`

### 实施步骤

#### 3.1：标题行重构

在 `<template>` 中找到 `.page-head` 区块，改为：

```vue
<div class="page-head">
  <div class="head-left">
    <h1 class="page-title">用量汇总</h1>
    <div class="page-sub">
      统计时区 {{ tzLabel }} · 今天 {{ todayLabel }}
      <template v-if="drill"> · 已筛选 {{ drillLabel(drill) }}</template>
    </div>
  </div>
  <div class="head-right">
    <span v-if="stale" class="ts-pill ts-pill-warning">缓存数据 · 后台刷新中</span>
    <span v-else-if="loading" class="ts-pill ts-pill-info">刷新中…</span>
    <span v-else class="ts-pill ts-pill-success">● 已更新</span>
    <NButton size="small" secondary class="ts-focusable" @click="manualRefresh">刷新</NButton>
  </div>
</div>
```

样式改为：

```css
.page-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--ts-space-4);
  margin-bottom: var(--ts-space-4);
}

.head-left {
  flex: 1;
}

.head-right {
  display: flex;
  align-items: center;
  gap: var(--ts-space-2);
}

.page-title {
  font-family: var(--ts-font-display);
  font-size: 28px;
  font-weight: 700;
  line-height: 1.2;
  letter-spacing: -0.02em;
  margin: 0;
  color: var(--ts-text);
}

.page-sub {
  font-size: 12px;
  line-height: 1.4;
  color: var(--ts-text-muted);
  margin-top: var(--ts-space-1);
}
```

删除原来单独渲染的 `<NTag v-if="stale">` 和 `<NTag v-else-if="loading">`（它们在数据加载区，现在挪到标题行了）。

#### 3.2：筛选栏改造

找到 `.filter-row`，将来源按钮改为分段控件：

```vue
<div class="filter-row">
  <SegmentedControl
    v-model="agent"
    :options="agentOptions"
    aria-label="数据来源"
  />
  <SegmentedControl
    v-model="by"
    :options="dimOptions"
    aria-label="聚合维度"
  />
  <DateRangeSelect v-model:value="range" :tz="tz" />
  <NSelect
    :value="tz"
    :options="TZ_OPTIONS"
    size="small"
    class="tz-select"
    aria-label="统计时区"
    @update:value="(v: string) => (tz = v)"
  />
</div>
```

在 `<script setup>` 中，`agentOptions` 改为：

```ts
const agentOptions = [
  { value: "all" as const, label: "全部", icon: "◎" },
  { value: "claude" as const, label: "Claude Code", icon: "◉" },
  { value: "codex" as const, label: "Codex", icon: "⊙" },
];
```

删除原来的 `.agent-row` 和 `.agent-btn` 样式，`.filter-row` 样式保持不变（已经是 flex wrap）。

#### 3.3：异常与错误改为内联通知条

找到 `<NAlert v-for="s in sourceStatus">` 和 `<NAlert v-if="summaryError">`，改为：

```vue
<!-- 来源异常 -->
<div
  v-for="s in sourceStatus.filter((x) => x.state !== 'ready')"
  :key="s.agent"
  class="ts-notice"
  style="margin-bottom: var(--ts-space-3)"
>
  <span class="ts-notice-icon">⚠️</span>
  <span class="ts-notice-content">
    <template v-if="s.state === 'disabled'">
      {{ AGENT_LABEL[s.agent] ?? s.agent }} 已在设置中停用，不参与统计。
    </template>
    <template v-else-if="s.state === 'missing'">
      {{ AGENT_LABEL[s.agent] ?? s.agent }} 数据目录不存在（{{ s.dir }}）。
    </template>
    <template v-else>
      {{ AGENT_LABEL[s.agent] ?? s.agent }} 目录存在但没有发现会话日志。
    </template>
  </span>
  <a v-if="s.state !== 'disabled'" class="ts-notice-action" href="#" @click.prevent="() => {}">
    去设置 ›
  </a>
</div>

<!-- 汇总错误 -->
<div v-if="summaryError" class="ts-notice" style="margin-bottom: var(--ts-space-3)">
  <span class="ts-notice-icon">❌</span>
  <span class="ts-notice-content">汇总加载失败：{{ summaryError }}</span>
  <button class="ts-notice-action" @click="refresh">重试</button>
</div>

<!-- 明细错误 -->
<div v-if="eventsError" class="ts-notice" style="margin-bottom: var(--ts-space-3)">
  <span class="ts-notice-icon">❌</span>
  <span class="ts-notice-content">明细加载失败：{{ eventsError }}</span>
  <button class="ts-notice-action" @click="loadEvents()">重试</button>
</div>
```

删除所有 `<NAlert>` 的导入和使用。

#### 3.4：导入 SegmentedControl

在 `<script setup>` 顶部加：

```ts
import SegmentedControl from "../components/SegmentedControl.vue";
```

#### 3.5：更新测试

`Dashboard.test.ts` 中，来源按钮选择器从 `.agent-btn` 改为分段控件的 `role="radio"`，异常断言从 `NAlert` 改为 `.ts-notice`。

### 验收标准

- `pnpm --dir frontend typecheck` 通过
- `pnpm --dir frontend test` 全绿
- 标题行右侧能看到状态胶囊
- 筛选栏所有控件高度对齐（32px）
- 来源是"图标 + 文字"的分段控件，不是纯图标方块

---

## Task 4：指标卡重构

### 目标

- 左侧主读数"估算费用"（44px），下方小字"USD · 估算值，非账单"
- 右侧三个次读数（总 token、请求数、缓存命中率，26px），之间用发丝线分隔
- 含未计价 token 时，在费用旁显示警告色胶囊"含未计价 token"
- 命中率公式放进 tooltip（focus 可达），不直接铺在卡片上
- 卡片底部新增四类 token 分项比例条（6px 高、圆角 3px，按占比分段）
- 比例条下方一行图例：色点 + 名称 + 数值

### 文件清单

- **修改** `frontend/src/components/SummaryCards.vue`
- **修改** `frontend/src/components/SummaryCards.test.ts`

### 实施步骤

#### 4.1：模板重构

整个 `<template>` 改为：

```vue
<template>
  <section class="metric-card ts-card" aria-label="用量指标">
    <div class="metric-row">
      <!-- 主读数：费用 -->
      <div class="metric-main">
        <div class="metric-label">
          估算费用
          <span v-if="totals.unknown_pricing" class="ts-pill ts-pill-warning">
            含未计价 token
          </span>
        </div>
        <div class="metric-value ts-num">
          <template v-if="costUnknownOnly">未知†</template>
          <template v-else>{{ costText }}</template>
        </div>
        <div class="metric-unit">USD · 估算值，非账单</div>
      </div>

      <!-- 次读数：三项 -->
      <div class="metric-secondary">
        <div class="metric-item">
          <div class="metric-label">总 token</div>
          <div class="metric-value ts-num">{{ fmtNum(total) }}</div>
          <div class="metric-unit">≈ {{ wan }} 万 tokens</div>
        </div>
        <div class="metric-sep" />
        <div class="metric-item">
          <div class="metric-label">请求数</div>
          <div class="metric-value ts-num">{{ fmtNum(totals.requests) }}</div>
          <div class="metric-unit">次请求</div>
        </div>
        <div class="metric-sep" />
        <div class="metric-item">
          <NTooltip placement="bottom">
            <template #trigger>
              <div class="metric-label" tabindex="0" style="cursor: help">
                缓存命中率
              </div>
            </template>
            <div style="max-width: 320px">
              命中率 = 缓存读 ÷（新增输入 + 缓存读）。
              <br />缓存读直接复用上下文，消耗 token 数计入分母但费用通常为零或极低。
            </div>
          </NTooltip>
          <div class="metric-value ts-num">
            {{ hitRate == null ? "N/A" : `${hitRate.toFixed(1)}%` }}
          </div>
          <div class="metric-unit">缓存读占比</div>
        </div>
      </div>
    </div>

    <!-- 分项比例条 -->
    <div class="parts-bar-container">
      <div v-if="total > 0" class="parts-bar">
        <span
          v-for="p in parts"
          :key="p.kind"
          :class="`bar-segment bar-${p.kind}`"
          :style="{ width: `${((p.value / total) * 100).toFixed(2)}%` }"
          :aria-label="`${p.label} ${fmtNum(p.value)}`"
        />
      </div>
      <div v-else class="parts-bar parts-bar-empty" />
    </div>

    <!-- 图例 -->
    <div class="parts-legend" aria-label="token 分项">
      <span v-for="p in parts" :key="p.kind" class="legend-item">
        <span class="legend-dot" :class="`part-${p.kind}`" aria-hidden="true" />
        <span class="legend-label">{{ p.label }}</span>
        <span class="legend-value ts-num">{{ fmtNum(p.value) }}</span>
      </span>
    </div>
  </section>
</template>
```

#### 4.2：脚本不变

`<script setup>` 保持原样（`total`、`wan`、`hitRate`、`costText`、`costUnknownOnly`、`parts` 计算属性都不变），只需加一行导入：

```ts
import { NTooltip } from "naive-ui";
```

#### 4.3：样式重写

整个 `<style scoped>` 改为：

```css
.metric-card {
  /* ts-card 已提供 background / border-radius / padding / shadow */
}

.metric-row {
  display: flex;
  gap: var(--ts-space-6);
  align-items: flex-start;
}

.metric-main {
  flex: 0 0 auto;
  min-width: 200px;
}

.metric-secondary {
  flex: 1;
  display: flex;
  gap: var(--ts-space-4);
  align-items: flex-start;
}

.metric-item {
  flex: 1;
  min-width: 0;
}

.metric-sep {
  width: 1px;
  height: 48px;
  background: var(--ts-separator);
  flex-shrink: 0;
  align-self: center;
}

.metric-label {
  font-size: 12px;
  font-weight: 500;
  line-height: 1.4;
  color: var(--ts-text-secondary);
  display: flex;
  align-items: center;
  gap: var(--ts-space-2);
  margin-bottom: var(--ts-space-1);
}

.metric-value {
  font-family: var(--ts-font-display);
  font-size: 26px;
  font-weight: 600;
  line-height: 1.2;
  letter-spacing: -0.02em;
  color: var(--ts-text);
}

.metric-main .metric-value {
  font-size: 44px;
  letter-spacing: -0.03em;
}

.metric-unit {
  font-size: 12px;
  line-height: 1.4;
  color: var(--ts-text-muted);
  margin-top: 2px;
}

.parts-bar-container {
  margin-top: var(--ts-space-5);
}

.parts-bar {
  display: flex;
  height: 6px;
  border-radius: 3px;
  overflow: hidden;
  background: var(--ts-fill);
}

.parts-bar-empty {
  background: var(--ts-fill);
}

.bar-segment {
  height: 100%;
}

.bar-input {
  background: var(--ts-chart-input);
}
.bar-output {
  background: var(--ts-chart-output);
}
.bar-cache_write {
  background: var(--ts-chart-cache-write);
}
.bar-cache_read {
  background: var(--ts-chart-cache-read);
}

.parts-legend {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--ts-space-4);
  margin-top: var(--ts-space-3);
  font-size: 13px;
}

.legend-item {
  display: inline-flex;
  align-items: center;
  gap: var(--ts-space-2);
}

.legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.part-input {
  background: var(--ts-chart-input);
}
.part-output {
  background: var(--ts-chart-output);
}
.part-cache_write {
  background: var(--ts-chart-cache-write);
}
.part-cache_read {
  background: var(--ts-chart-cache-read);
}

.legend-label {
  color: var(--ts-text-secondary);
}

.legend-value {
  color: var(--ts-text);
  font-weight: 600;
}

@media (max-width: 1024px) {
  .metric-row {
    flex-direction: column;
    gap: var(--ts-space-5);
  }
  .metric-secondary {
    width: 100%;
  }
}
```

#### 4.4：更新测试

`SummaryCards.test.ts`：
- 比例条宽度断言：`.parts-bar .bar-segment` 检查 `width` 样式
- 全 0 时空槽：`.parts-bar-empty` 存在
- 未知价格：检查胶囊 `.ts-pill-warning` 显示"含未计价 token"

### 验收标准

- 费用是最大的数字（44px），其他三项 26px
- 比例条按 token 占比显示四种颜色
- 全 0 时比例条显示为空槽（灰色底）
- 缓存命中率标签 hover 或 focus 能打开 tooltip
- 含未计价 token 时费用旁有警告色胶囊

---

## Task 5：卡片统一（趋势/聚合/明细）

### 目标

- 三个区块（趋势、聚合、明细）统一用 `.ts-card` 样式
- 去掉行内 `margin-top: 12px`，改为区块间距 `20px`
- 图表：去坐标轴线、柱顶圆角 4px、圆点图例
- 表格：无竖线无外框、发丝线行分隔、40px 行高、可点击行 `›` 指示、合计行上方粗线

### 文件清单

- **修改** `frontend/src/components/TrendChart.vue`
- **修改** `frontend/src/components/UsageTable.vue`
- **修改** `frontend/src/components/EventTable.vue`
- **修改** `frontend/src/views/Dashboard.vue`

### 实施步骤

#### 5.1：Dashboard 区块间距

在 `Dashboard.vue` 的 `<style scoped>` 最后加：

```css
.metric-card,
.ts-card,
.n-card {
  margin-bottom: var(--ts-space-5);
}

.metric-card:last-child,
.ts-card:last-child,
.n-card:last-child {
  margin-bottom: 0;
}
```

删除所有行内 `style="margin-top: 12px"` 和 `style="margin-bottom: 12px"`。

#### 5.2：TrendChart 卡片化

`TrendChart.vue` 模板外层加 `.ts-card`：

```vue
<template>
  <section class="ts-card">
    <div class="chart-head">
      <span class="chart-title">{{ titleText }}</span>
      <span class="chart-state">{{ stateText }}</span>
      <button
        type="button"
        class="summary-toggle ts-focusable"
        :aria-expanded="showSummary"
        @click="showSummary = !showSummary"
      >
        数据摘要
      </button>
    </div>
    <!-- 其余不变 -->
  </section>
</template>
```

样式中 `.trend-chart` 改为仅控制内部，不再定义 padding / border / background（`.ts-card` 已提供）。

图表配置中，柱子加圆角：

```ts
series: series.map((s) => ({
  name: s.name,
  type: "bar",
  stack: "tokens",
  barMaxWidth: 36,
  data: s.values,
  itemStyle: {
    borderRadius: isDay.value ? [4, 4, 0, 0] : [0, 4, 4, 0], // 柱顶圆角
  },
})),
```

图例改为圆点：

```ts
legend: {
  top: 0,
  textStyle: { color: t.legendText, fontSize: 12 },
  icon: "circle",
  itemWidth: 8,
  itemHeight: 8,
},
```

#### 5.3：UsageTable 表格样式

表格外层已经在 `Dashboard.vue` 中被 `NCard` 包裹，无需额外改动，只需调整 Naive DataTable 的样式：

在 `naiveTheme.ts` 的 `DataTable` 覆盖中确认以下配置（Task 1 已设置，确认一遍）：

```ts
DataTable: {
  borderColor: "transparent",
  borderRadius: "0",
  thColor: c.surface,
  tdColor: c.surface,
  tdColorHover: c.fillHover,
  thPaddingMedium: "12px 16px",
  thPaddingSmall: "10px 12px",
  tdPaddingMedium: "12px 16px",
  tdPaddingSmall: "10px 12px",
  thFontWeight: "500",
},
```

在 `UsageTable.vue` 的表格列定义中，可点击行的 `render` 加 `›` 指示：

```ts
{
  key: "key",
  title: report.by === "day" ? "日期" : ...,
  render: (row) => {
    const isTotal = row.key === "合计";
    return h(
      "span",
      { style: { fontWeight: isTotal ? "600" : "normal" } },
      [
        row.label ?? row.key,
        !isTotal && " ›", // 可点击行右侧加箭头
      ]
    );
  },
},
```

合计行上方粗线：在 `.total-row` 样式中加：

```css
.total-row {
  border-top: 1px solid var(--ts-separator-strong);
}
.total-row strong {
  font-weight: 700;
}
```

表格行高：在 `naiveTheme.ts` 中加（如果没有的话）：

```ts
DataTable: {
  // ... 已有配置
  tdPaddingMedium: "12px 16px", // 上下 12px → 行高约 40px（13px 字 + 行高 1.45 + padding）
  tdPaddingSmall: "10px 12px",
},
```

#### 5.4：EventTable 同理

`EventTable.vue` 已经在 `NCard` 中，表格样式靠 `naiveTheme.ts` 统一覆盖，只需确认没有行内覆盖样式。

### 验收标准

- 指标卡、趋势卡、聚合卡、明细卡之间间距一致（20px）
- 图表柱子顶部有圆角，图例是圆点
- 表格没有竖线和外框，行与行之间只有发丝线
- 可点击行右侧有 `›` 指示
- 合计行上方有一条较粗的分隔线

---

## Task 6：状态横幅与设置页（暂缓）

**说明**：`Settings.vue` 在工作区有未提交改动，暂不修改。`PricingStatusBanner` 已经是独立组件，可以单独改为内联通知条样式，但与 Task 3 的异常通知重复，建议合并到 Task 3 一起做。

如果要做：

- `PricingStatusBanner.vue` 模板改为 `.ts-notice` 结构（图标 + 内容 + 操作）
- 删除 `NAlert` 使用，改为手写 div

---

## Task 7：验收与截图

### 真机走查清单

**执行命令**：

```powershell
$env:HTTP_PROXY = "http://127.0.0.1:7897"; $env:HTTPS_PROXY = "http://127.0.0.1:7897"
.\frontend\node_modules\.bin\tauri dev
```

**走查项**：

1. **吸顶玻璃效果**：滚动页面，内容从导航栏下方滚过时，导航栏有模糊效果（macOS / Windows 11 可见，Win 10 退化为实色）
2. **分段控件动画**：切换页面/主题/来源/维度时，选中块平移流畅（200ms）
3. **浅色与深色**：
   - 浅色：画布浅灰 `#F5F5F7`，卡片纯白，有阴影分层
   - 深色：画布近黑 `#0F0F11`，卡片深灰 `#1C1C1E`，有细描边
4. **窗口尺寸**：
   - 1280×820：所有内容正常显示
   - 980×620（最小）：筛选栏换行，指标区变 2×2，不出现横向滚动
5. **指标卡**：
   - 费用最大（44px），其余三项 26px
   - 比例条按占比显示四色，全 0 时空槽
   - 缓存命中率标签 hover 打开 tooltip
6. **长名称**：模型名、项目名超长时省略，tooltip 显示全名
7. **数据状态**：
   - 无缓存：首次启动显示"尚未获取定价"横幅
   - 部分价格：含未计价 token 时费用旁有警告胶囊
   - 零值与未知：零显示 `0`，未知显示 `—` 或 `未知†`，不混用
8. **键盘操作**：
   - Tab 顺序符合视觉顺序
   - 分段控件方向键切换
   - 表格行 Enter 下钻
   - tooltip 可通过 focus 打开
   - Escape 关闭浮层
9. **系统缩放**：100%、125%、150% 下核心数字和操作不截断

**截图要求**：

- 浅色 1280×820：首屏（指标卡+趋势+聚合表前几行）
- 深色 1280×820：同上
- 980×620：筛选栏换行效果
- 指标卡特写：比例条与图例
- 来源异常横幅：`.ts-notice` 样式

截图存到 `docs/plans/active/screenshots/2026-10-06/`，在 `2026-10-06-design-system-visual-qa.md` 中记录走查结果。

---

## 门禁与提交规范

每个 Task 完成后：

1. `pnpm --dir frontend typecheck` 通过
2. `pnpm --dir frontend format:check` 通过
3. `pnpm --dir frontend test` 全绿（107 passed）
4. `pnpm --dir frontend build` 通过
5. 独立 commit，message 格式：`feat(界面): Task N - 简短描述`
6. Push 到当前分支 `docs/product-review-plan`

全部完成后，真机走查（Task 7），截图验收，再合并到 main。

---

## 常见问题

**Q：为什么不一次性改完再提交？**
A：每个 Task 独立可测试、可回滚。Task 3 失败不影响 Task 1-2 已落地。

**Q：Naive UI 组件样式覆盖不生效？**
A：优先级：组件 scoped style < Naive themeOverrides < 组件 props。用 `::v-deep()` 或在 `naiveTheme.ts` 中统一覆盖。

**Q：分段控件选中块位置不对？**
A：`nextTick(updateThumb)` 确保 DOM 更新后再计算位置；窗口 resize 时需要重新计算。

**Q：深色模式描边看不见？**
A：`.ts-card` 已统一用 `var(--ts-glass-stroke)` 描边 + `--ts-glass-highlight` 顶部高光，组件里不要再单独写深色描边。

**Q：玻璃卡片里的表格/图表看着发虚？**
A：卡片本体是半透明玻璃（`--ts-surface` 75%）。表格主体、ECharts 绘图区、费用公式区必须包一层 `.ts-card-solid`（实色 `--ts-surface-solid`）；Naive DataTable 的 `thColor/tdColor` 已在 `naiveTheme.ts` 指向实色，不要改回透明。

**Q：玻璃没效果、看起来像灰条？**
A：玻璃依赖 `body` 上的固定柔光（`--ts-canvas-aurora`）。不要在 App 壳、`.app-content` 或页面根节点上加不透明背景色，否则会挡住柔光。

**Q：卡片能不能再嵌套一层 `.ts-card`？**
A：不能。`backdrop-filter` 嵌套会叠加模糊、拖慢渲染，内部分区一律用 `.ts-card-solid` 或发丝线 `--ts-separator`。

**Q：比例条宽度加起来不是 100%？**
A：浮点误差，用 `toFixed(2)` 并确保最后一段 `flex: 1` 或宽度 `calc(100% - 已用宽度)`。

---

## 执行 Agent 的注意事项

1. **严格按顺序**：Task 3 → 4 → 5 → 7（Task 6 暂缓）
2. **每个 Task 单独 commit**：不要合并提交
3. **测试先行**：改完测试选择器再改实现，确保测试覆盖到新结构
4. **只改列出的文件**：不要顺手重构无关代码
5. **保持业务逻辑不变**：统计公式、价格匹配、缓存、下钻行为都不改
6. **遇到工作区冲突**：如 `Settings.vue` 有未提交改动，跳过该文件，在 commit message 中说明

完成后用户可以：
- 本地 `tauri dev` 预览
- 截图发到其他渠道评审
- 合并到 main 并打 tag 发布
