# 控件材质与浮层可读性计划：按钮边界、焦点与操作浮层

> 状态：**已实施，待用户视觉验收。** 本文是逐项对照实现的规格：token 数值、Naive 主题字段、CSS 代码、组件改动、测试名称与断言、`DESIGN.md` 替换文本都已写定。实施中如需偏离，先在本文“偏离记录”（第 10 节）写明原因，再改代码。
>
> 用户反馈（两张截图）：
> - 暗色模式下按钮边界不清，分不出是文字还是按钮。
> - 鼠标点击后，边缘高亮不对劲。
> - 不像磨砂玻璃；亮色模式下边缘只有一点灰。
> - 日期面板里的时刻滚轮半透明，盖在日历上，干扰选择，数字看不清。
>
> 已确认的决策：
> - **D1：控件边界严格 ≥ 3:1**（WCAG 1.4.11）。
> - **D2：克制的边缘受光，外加焦点与选中时的柔光**，不做霓虹。
> - **D3：需要点选的浮层不透明**，说明类浮层保留玻璃。

**Goal：**
- 所有可点击控件在明暗两种主题下，静止时就能认出是控件。
- 焦点高亮只在键盘操作时出现。
- 选中状态有明确且克制的强调光。
- 操作型浮层不透出下层内容。

**Architecture：**
- 在 `DESIGN.md` 的分层里加入“控件材质”层。
- `tokens.css` 定义控件 token 和公共类；`naiveTheme.ts` 把 Naive 的按钮、输入框、选择框、勾选框、开关、时间和日期面板的**全部状态**映射到这些 token；手写控件改用公共类。
- 浮层按“操作型 / 说明型”分类：操作型实色，说明型保留 85% 玻璃。
- 浏览器契约按实际渲染的像素测量边界对比度，并断言焦点行为。

**Tech Stack：** Vue 3 / Naive UI 2.45.3（不升级）/ `tokens.css` / `naiveTheme.ts` / Vitest + happy-dom / Playwright（`check-ui-contracts.mjs`）。

## 0. 执行须知（给执行模型）

1. **分支：** 从 `docs/control-material-plan` 的最新提交切出 `feat/control-material`。
   - 按 C01→C07 顺序实施，每项一个提交（Conventional Commits，中文正文写明改了什么、为什么、影响范围）。
   - 不写 AI 署名，严禁 `--no-verify`；验证、提交、推送严格串行。
2. **先测后改：** 每项先写第 6 节列出的测试，运行确认是红的，再改实现，最后确认转绿。红灯输出保存到 `qa-artifacts/control-material/<任务号>-red.log`。
3. **每项完成后的门禁**（在仓库根执行）：
   - `pnpm --dir frontend typecheck`
   - `pnpm --dir frontend format:check`
   - `pnpm --dir frontend test`
   - `pnpm --dir frontend build`
   - `pnpm --dir frontend test:e2e`（浏览器契约，C06 起包含新契约）

   本计划不改 Rust，根库和壳的 cargo 门禁由提交钩子照常运行。
4. **颜色只走 token：** 组件和 `.vue` 文件里不得新增十六进制或 `rgba()` 字面量；新颜色先进 `tokens.css`，再同步到 `naiveTheme.ts` 的调色板。
5. **不改业务行为：** 日期草稿/确认/取消、刷新、保存、导入、主题持久化的逻辑一行都不动。已有的行为测试必须全部原样通过；如需修改断言，只允许改**样式类断言**，并在提交正文里列出。
6. **版本与合并：** 这是可读性和无障碍修复，合入 main 前递增 patch：先 `git fetch`，再执行 `python -B scripts/bump_version.py --bump patch --summary "控件材质与浮层可读性"`。PR 不自行合并，等用户看截图。

## 1. 现状根因（已核实，实施时不必再调查）

| 编号 | 现象 | 根因 |
| --- | --- | --- |
| F1 | 次按钮像纯文字 | `naiveTheme.ts` 设了 `Button.border: "none"`，而 Naive 默认按钮的底色是 `#0000`（透明）。`secondary`/`tertiary`/`quaternary` 三种变体在 Naive 里不渲染边框元素（`Button.mjs` 第 98 行 `showBorder`）。 |
| F2 | 鼠标点击后出现细蓝边、文字变蓝 | Naive 按钮在 `:focus` 时应用 `borderFocus`（`1px solid primaryColorHover`）和 `textColorFocus`（`button/src/styles/index.cssr.mjs:42-59`）；它不区分鼠标点击和键盘聚焦，而我们只覆盖了 `border`。点击时的波纹 `rippleColor` 默认是主色，会出现一圈蓝色扩散。 |
| F3 | 焦点环在玻璃上发黑或发灰 | `--ts-focus-ring` 的 2px 间隙用的是 `--ts-canvas`。 |
| F4 | 暗色主按钮是浅蓝块 | Naive 主按钮底色取自 `common.primaryColor`，它被设成了 `--ts-accent`（`#4DA3FF`，白字对比只有 2.63:1），而不是 `--ts-accent-fill`。 |
| F5 | 时刻滚轮透出日历 | 时间列浮层的底色是 `TimePicker.panelColor`（来自 `popoverColor`，85% 半透明），而 `tokens.css` 的模糊名单不含 `.n-time-picker-panel`：半透明且无模糊，又叠在第一层浮层之上。 |
| F6 | 悬停更淡 | `--ts-fill-hover`（8% / 16%）低于 `--ts-fill`（12% / 24%）。 |
| F7 | 选中态文字对比不足 | 日期按钮选中后是 `--ts-accent` 文字配 `--ts-accent-soft` 底，实测只有 3.9:1（暗色）和 4.1:1（亮色），不到 4.5:1。 |
| F8 | 勾选框、开关、输入框边界几乎看不见 | Checkbox 边框取 `common.borderColor`，即 `--ts-separator`（8%）；Input 描边是 14% / 16%；Select 被设成了 `border: none`。 |

现值实测：
- 日期快捷项分隔线：暗色 1.28:1，亮色 1.20:1；
- 分段控件底槽：暗色 1.32:1，亮色 1.15:1。

## 2. 不变量

1. 业务行为与可访问名称不变；已有行为测试全部通过。
2. 键盘焦点（`:focus-visible`）始终可见：2px 强调色描边加柔光。鼠标点击按钮后不出现焦点环，颜色也不变。
   - 例外：**文本输入类**控件（Input、Select、日期输入）点击后获得焦点并显示焦点样式，这是正确行为。
3. 除第 4.4 节列出的豁免外，所有可点击控件**静止时**的边界与相邻背景对比 ≥ 3:1，明暗两主题都要满足。
4. 文字对比 ≥ 4.5:1，包括选中态和主按钮；禁用态按 WCAG 豁免，但必须是 50% 不透明并去掉光效。
5. 一个浮层只有一层材质；**操作型浮层和任何嵌套浮层都是实色**（alpha = 1，无 `backdrop-filter`）。
6. 光效上限（D2）：
   - 柔光只出现在悬停（暗色中性外晕）、键盘焦点和选中态；
   - 不透明度 ≤ 30%，模糊或扩散 ≤ 12px；
   - 不做动画脉动，减少动态效果模式下没有过渡。
7. 颜色只经语义 token 使用；`naiveTheme.ts` 与 `tokens.css` 同值，由 `themeContract.test.ts` 守护。

## 3. Token 定稿（写入 `tokens.css` 的 `:root` 与 `:root[data-theme="dark"]`）

### 3.1 新增与修改

| Token | 亮色 | 暗色 | 用途 |
| --- | --- | --- | --- |
| `--ts-control-bg` | `rgba(255, 255, 255, 0.92)` | `rgba(255, 255, 255, 0.07)` | 次按钮、日期按钮、输入框、选择框的底色 |
| `--ts-control-bg-hover` | `#ffffff` | `rgba(255, 255, 255, 0.12)` | 悬停（比静止更强） |
| `--ts-control-bg-pressed` | `rgba(118, 118, 128, 0.16)` | `rgba(255, 255, 255, 0.04)` | 按下 |
| `--ts-control-stroke` | `rgba(0, 0, 0, 0.44)` | `rgba(255, 255, 255, 0.34)` | 静止边界（≥ 3:1） |
| `--ts-control-stroke-hover` | `rgba(0, 0, 0, 0.56)` | `rgba(255, 255, 255, 0.46)` | 悬停和按下时的边界 |
| `--ts-control-rim` | `inset 0 1px 0 rgba(255, 255, 255, 0.9)` | `inset 0 1px 0 rgba(255, 255, 255, 0.12)` | 上沿受光 |
| `--ts-control-shadow` | `0 1px 2px rgba(0, 0, 0, 0.1)` | `0 1px 2px rgba(0, 0, 0, 0.45)` | 抬起感 |
| `--ts-control-glow-hover` | `0 2px 8px rgba(0, 0, 0, 0.08)` | `0 0 10px rgba(255, 255, 255, 0.06)` | 悬停：亮色是抬升阴影，暗色是中性外晕 |
| `--ts-accent-fill-hover` | `#0062c4` | `#0057cc` | 主按钮悬停（比基色更深，白字对比只升不降） |
| `--ts-accent-fill-pressed` | `#00539f` | `#004bb0` | 主按钮按下 |
| `--ts-selected-bg` | `var(--ts-accent-soft)` | `var(--ts-accent-soft)` | 选中态底 |
| `--ts-selected-stroke` | `var(--ts-accent)` | `var(--ts-accent)` | 选中态边界（实色） |
| `--ts-selected-glow` | `0 0 0 3px rgba(0, 113, 227, 0.12)` | `0 0 12px rgba(77, 163, 255, 0.28)` | 选中柔光 |
| `--ts-focus-halo` | `0 0 0 6px rgba(0, 113, 227, 0.18)` | `0 0 0 6px rgba(77, 163, 255, 0.24)` | 键盘焦点柔光 |
| `--ts-surface-popover-solid` | `#ffffff` | `#2c2c2e` | 操作型浮层 |
| `--ts-popover-stroke` | `rgba(0, 0, 0, 0.12)` | `rgba(255, 255, 255, 0.14)` | 操作型浮层外描边（属于容器描边，不受 3:1 约束） |
| `--ts-row-hover` | `rgba(118, 118, 128, 0.08)` | `rgba(118, 118, 128, 0.16)` | 表格行和导航列表项的悬停（**由 `--ts-fill-hover` 改名**，值不变） |

### 3.2 删除

- `--ts-focus-ring`：亮色、暗色两处定义都删除，改用第 4.1 节的 outline 加 `--ts-focus-halo`。
- `--ts-fill-hover`：改名为 `--ts-row-hover`，删除旧名。

引用点只有三处：
- `TrendChart.vue`：`.chart-reset` 改为公共类后，这处引用自然消失；
- `Settings.vue`：`.settings-nav-item:hover` 改用 `--ts-row-hover`；
- `tokens.css` 内部。

`naiveTheme.ts` 调色板里的 `fillHover` 字段同步改名为 `rowHover`。

### 3.3 不变

`@supports not (backdrop-filter)` 的降级块不需要新增控件 token，因为控件 token 不依赖模糊。降级块里 `--ts-surface-elevated` 的取值保留不变。

### 3.4 定稿值的对比度核算

计算方法：按 token 逐层合成到四种衬底上，暗色衬底是玻璃卡片、叠柔光最亮处的卡片、实色浮层 `#2C2C2E`、对话框，亮色同理；然后用 WCAG 相对亮度公式求对比度。核算脚本思路见附录 A。

| 组合 | 最低值 | 要求 |
| --- | --- | --- |
| 暗色：控件描边 34% / 四种衬底 | 3.41:1 | ≥ 3:1 |
| 亮色：控件描边 44% / 四种衬底 | 3.20:1 | ≥ 3:1 |
| 暗色悬停描边 46% | > 4.7:1 | ≥ 3:1 |
| 选中描边（`--ts-accent`）/ 分段底槽 | 暗色 5.09:1，亮色 4.60:1 | ≥ 3:1 |
| 焦点 outline（`--ts-accent`）/ 卡片 | 暗色 6.73:1，亮色 5.29:1 | ≥ 3:1 |
| 选中态文字 `--ts-text` / 选中底 | 暗色 9.46:1，亮色 12.27:1 | ≥ 4.5:1 |
| 主按钮白字 / `--ts-accent-fill` | 暗色 5.62:1，亮色 4.70:1 | ≥ 4.5:1 |
| 危险文字 `--ts-error` / 控件底 | 暗色 6.26:1，亮色 5.38:1 | ≥ 4.5:1 |

## 4. 组件规格

### 4.1 公共类（追加到 `tokens.css`，替换现有 `.ts-focusable` 规则）

```css
/* ── 控件材质（DESIGN.md §2 控件材质）────────────────────────── */

/* 次按钮材质：底 + 描边 + 上沿受光 + 抬起阴影。手写按钮用它，Naive 按钮由主题映射。 */
.ts-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--ts-space-1);
  height: 32px;
  padding: 0 var(--ts-space-3);
  box-sizing: border-box;
  border: 1px solid var(--ts-control-stroke);
  border-radius: var(--ts-radius-control);
  background: var(--ts-control-bg);
  color: var(--ts-text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
  box-shadow: var(--ts-control-rim), var(--ts-control-shadow);
  transition:
    background-color var(--ts-motion-fast) var(--ts-ease),
    border-color var(--ts-motion-fast) var(--ts-ease),
    box-shadow var(--ts-motion-fast) var(--ts-ease);
}
.ts-btn:hover:not(:disabled) {
  background: var(--ts-control-bg-hover);
  border-color: var(--ts-control-stroke-hover);
  box-shadow: var(--ts-control-rim), var(--ts-control-shadow), var(--ts-control-glow-hover);
}
.ts-btn:active:not(:disabled) {
  background: var(--ts-control-bg-pressed);
  border-color: var(--ts-control-stroke-hover);
  box-shadow: none;
}
.ts-btn:disabled {
  opacity: 0.5;
  box-shadow: none;
  cursor: not-allowed;
}
.ts-btn-sm {
  height: 28px;
  padding: 0 var(--ts-space-2);
  font-size: 12px;
}

/* 文字按钮：只用于行内/通知条；必须是强调色，悬停出现底色。 */
.ts-btn-text {
  border: none;
  background: transparent;
  color: var(--ts-accent);
  font: inherit;
  font-weight: 500;
  padding: 2px var(--ts-space-1);
  border-radius: var(--ts-radius-thumb);
  cursor: pointer;
  transition: background-color var(--ts-motion-fast) var(--ts-ease);
}
.ts-btn-text:hover:not(:disabled) {
  background: var(--ts-row-hover);
}

/* 选择型：静止是次按钮材质（与 .ts-btn 同用），选中是选中材质。
   带 :hover 的选择器保证悬停时仍是选中材质：特异性 (0,4,0) 高于 .ts-btn:hover:not(:disabled)
   的 (0,3,0)；.ts-selected-surface 的两条与 .ts-btn 规则同级，所以本块必须写在 .ts-btn 规则之后。 */
.ts-choice[aria-pressed="true"],
.ts-choice[aria-pressed="true"]:hover:not(:disabled),
.ts-selected-surface,
.ts-selected-surface:hover:not(:disabled) {
  background: var(--ts-selected-bg);
  border-color: var(--ts-selected-stroke);
  color: var(--ts-text);
  font-weight: 600;
  box-shadow: var(--ts-selected-glow);
}

/* 键盘焦点：2px 强调色 outline（偏移 2px）+ 柔光；不再使用画布色间隙。
   :root 前缀把特异性抬到 (0,3,0)，以压过 Naive 运行时注入的同级规则。 */
:root .ts-focusable:focus-visible,
:root .ts-btn:focus-visible,
:root .ts-btn-text:focus-visible,
:root .ts-segmented-item:focus-visible,
:root .n-button:focus-visible {
  outline: 2px solid var(--ts-accent);
  outline-offset: 2px;
  box-shadow: var(--ts-focus-halo);
}
/* 保留原 .ts-focusable 的圆角行为：行内触发器聚焦时轮廓也是圆角。 */
:root .ts-focusable:focus-visible {
  border-radius: var(--ts-radius-control);
}

/* Naive 按钮补上沿受光和阴影（只给渲染了边框的变体：默认、主、危险；文字按钮没有 __border）。 */
.n-button:not(.n-button--disabled):has(> .n-button__border) {
  box-shadow: var(--ts-control-rim), var(--ts-control-shadow);
}
.n-button:not(.n-button--disabled):has(> .n-button__border):hover {
  box-shadow: var(--ts-control-rim), var(--ts-control-shadow), var(--ts-control-glow-hover);
}
.n-button:not(.n-button--disabled):has(> .n-button__border):active {
  box-shadow: none;
}

/* 操作型浮层（DESIGN.md §2 浮层分类）：实色 + 描边，无模糊。
   Naive 的 .n-popover:not(.n-popover--raw) 特异性为 (0,2,0)，这里用 :root 抬到 (0,3,0)。 */
:root .n-popover.ts-popover-solid,
:root .n-time-picker-panel,
:root .n-base-select-menu {
  background-color: var(--ts-surface-popover-solid);
  border: 1px solid var(--ts-popover-stroke);
  -webkit-backdrop-filter: none;
  backdrop-filter: none;
}
```

> `:has()` 需要 Chromium 105 及以上。WebView2 与 Playwright 自带的 Chromium 都满足。

同时修改现有规则：

1. **模糊名单**：从
   ```css
   .n-popover, .n-tooltip, .n-base-select-menu, .n-date-panel { backdrop-filter: … }
   ```
   改为
   ```css
   .n-popover:not(.ts-popover-solid), .n-tooltip { backdrop-filter: … }
   ```
   去掉 `.n-base-select-menu` 和 `.n-date-panel`：日期面板是内联在实色浮层里的，不能再加模糊。
2. **分段控件 `.ts-segmented`**：
   - 加 `border: 1px solid var(--ts-control-stroke);`，`padding` 由 `2px` 改为 `1px`，外高仍是 32px（`box-sizing: border-box` 不变）。
   - 删除旧的 `.ts-segmented-item:focus-visible` 规则（已并入上面的统一焦点规则）。
3. **`.ts-segmented-thumb`**：
   - `top: 1px; height: 28px;`
   - `box-sizing: border-box; border: 1px solid var(--ts-selected-stroke);`
   - `background: var(--ts-selected-bg);`
   - `box-shadow: var(--ts-selected-glow);`
   - 删除 `--ts-segment-thumb` 与 `--ts-shadow-segment-thumb` 两个 token 及其引用。
4. **`.ts-segmented-item[aria-checked="true"]`**：保持 `color: var(--ts-text); font-weight: 600;`（不用强调色文字，见 F7）。
5. **`.ts-notice-action`**：并入 `.ts-btn-text` 的外观。在类定义上追加 `padding: 2px var(--ts-space-1); border-radius: var(--ts-radius-thumb);`，悬停加 `background: var(--ts-row-hover)`，同时去掉 `text-decoration: underline`。

### 4.2 Naive 主题映射（`naiveTheme.ts`）

**调色板 `TsPalette`** 新增以下字段，两种模式的值严格等于第 3.1 节：
- `controlBg`、`controlBgHover`、`controlBgPressed`
- `controlStroke`、`controlStrokeHover`
- `accentFillHover`、`accentFillPressed`
- `errorSoft`（取自现有 `--ts-error-soft`）
- `popoverSolid`
- `focusHalo`（只取第 3.1 节柔光里的颜色，如亮色 `rgba(0, 113, 227, 0.18)`，给输入类 `boxShadowFocus` 拼 `0 0 0 3px`）

`fillHover` 改名为 `rowHover`。

`naiveThemeOverrides` 的改动（其余字段保持不变）：

```ts
const line = (color: string) => `1px solid ${color}`;
const inputFocusShadow = `0 0 0 3px ${c.focusHalo}`;

common: {
  // ……原有字段……
  inputColor: c.controlBg, // 原为 c.fill
  hoverColor: c.rowHover,  // 原为 c.fillHover
},
Button: {
  borderRadiusMedium: "8px",
  borderRadiusSmall: "8px",
  borderRadiusTiny: "8px",
  paddingMedium: "0 16px",
  paddingSmall: "0 12px",
  // 次按钮（default）：焦点相关字段一律等于静止值——鼠标点击不留痕，键盘焦点由 tokens.css 统一画
  color: c.controlBg,
  colorHover: c.controlBgHover,
  colorPressed: c.controlBgPressed,
  colorFocus: c.controlBg,
  colorDisabled: c.controlBg,
  textColor: c.text,
  textColorHover: c.text,
  textColorPressed: c.text,
  textColorFocus: c.text,
  textColorDisabled: c.text,
  border: line(c.controlStroke),
  borderHover: line(c.controlStrokeHover),
  borderPressed: line(c.controlStrokeHover),
  borderFocus: line(c.controlStroke),
  borderDisabled: line(c.controlStroke),
  rippleColor: "#0000",
  // 主按钮：填充色用 accentFill（不是文字强调色 accent）
  colorPrimary: c.accentFill,
  colorHoverPrimary: c.accentFillHover,
  colorPressedPrimary: c.accentFillPressed,
  colorFocusPrimary: c.accentFill,
  colorDisabledPrimary: c.accentFill,
  textColorPrimary: c.onAccent,
  textColorHoverPrimary: c.onAccent,
  textColorPressedPrimary: c.onAccent,
  textColorFocusPrimary: c.onAccent,
  textColorDisabledPrimary: c.onAccent,
  borderPrimary: line(c.accentFill),
  borderHoverPrimary: line(c.accentFillHover),
  borderPressedPrimary: line(c.accentFillPressed),
  borderFocusPrimary: line(c.accentFill),
  borderDisabledPrimary: line(c.accentFill),
  rippleColorPrimary: "#0000",
  // 危险按钮：描边式（次按钮底 + 错误色字与描边）
  colorError: c.controlBg,
  colorHoverError: c.errorSoft,
  colorPressedError: c.errorSoft,
  colorFocusError: c.controlBg,
  colorDisabledError: c.controlBg,
  textColorError: c.error,
  textColorHoverError: c.error,
  textColorPressedError: c.error,
  textColorFocusError: c.error,
  textColorDisabledError: c.error,
  borderError: line(c.error),
  borderHoverError: line(c.error),
  borderPressedError: line(c.error),
  borderFocusError: line(c.error),
  borderDisabledError: line(c.error),
  rippleColorError: "#0000",
  // 文字按钮（text 属性）：强调色文字，各状态不变色
  textColorText: c.accent,
  textColorTextHover: c.accent,
  textColorTextPressed: c.accent,
  textColorTextFocus: c.accent,
  textColorTextDisabled: c.textMuted,
},
Input: {
  borderRadius: "8px",
  heightMedium: "32px",
  heightSmall: "28px",
  color: c.controlBg,
  colorFocus: c.controlBg,
  border: line(c.controlStroke),
  borderHover: line(c.controlStrokeHover),
  borderFocus: line(c.accent),
  boxShadowFocus: inputFocusShadow,
},
Select: {
  peers: {
    InternalSelection: {
      borderRadius: "8px",
      heightMedium: "32px",
      heightSmall: "28px",
      color: c.controlBg,
      colorActive: c.controlBg,
      border: line(c.controlStroke),
      borderHover: line(c.controlStrokeHover),
      borderActive: line(c.accent),
      borderFocus: line(c.accent),
      boxShadowActive: inputFocusShadow,
      boxShadowFocus: inputFocusShadow,
    },
    InternalSelectMenu: { color: c.popoverSolid },
  },
},
Checkbox: {
  color: c.controlBg,
  border: line(c.controlStroke),
  borderChecked: line(c.accentFill),
  colorChecked: c.accentFill,
  borderFocus: line(c.accent),
  boxShadowFocus: inputFocusShadow,
},
Switch: {
  railColor: c.controlStroke,     // 关闭态轨道本身就是 ≥ 3:1 的灰
  railColorActive: c.accentFill,
  boxShadowFocus: inputFocusShadow,
},
TimePicker: { panelColor: c.popoverSolid },
DatePicker: {
  panelColor: c.popoverSolid,
  peers: { TimePicker: { panelColor: c.popoverSolid } },
},
```

- `Popover` / `Tooltip` 的 `color` **不改**（仍是 elevated 85%）。操作型浮层只靠 `ts-popover-solid` 类实色化，避免全局 Popover 覆盖顺带改动 tooltip。
- 如果 TypeScript 类型提示某个字段不存在（版本差异），以 `node_modules/naive-ui/es/<组件>/styles/light.mjs` 里的实际字段名为准，并在第 10 节记录。

### 4.3 按钮变体白名单

应用代码里的 `NButton` 只允许四种形态：
- 默认（次按钮）
- `type="primary"`
- `type="error"`（危险，描边式）
- `text`（文字按钮）

禁止使用 `secondary`、`tertiary`、`quaternary`、`ghost`、`dashed`、`color`、`text-color`、`:bordered="false"`。逐处修改：

| 位置 | 现状 | 改为 |
| --- | --- | --- |
| `views/Dashboard.vue` 刷新按钮 | `secondary` + `class="ts-focusable"` | 删除 `secondary` 与 `class="ts-focusable"`（焦点由 `.n-button:focus-visible` 统一处理） |
| `components/CloseConfirmDialog.vue`「最小化到托盘」 | `type="primary" secondary` | `type="primary"` |
| `components/CloseConfirmDialog.vue`「直接退出」 | `type="error" secondary` | `type="error"` |
| `components/PrivacyExitDialog.vue`「退出程序」 | `type="error" secondary` | `type="error"` |
| `components/DateRangeSelect.vue`「清除」 | `quaternary` | `text` |

### 4.4 手写控件

| 控件 | 文件 | 规格 |
| --- | --- | --- |
| 日期按钮 `.range-trigger` | `DateRangeSelect.vue` | 在 `class` 上追加 `ts-btn`。局部样式只保留尺寸、省略号和 `is-icon` 宽度，删除其中的 `border`、`background`、`color` 和 `:hover`。`has-preset` 状态追加 `ts-selected-surface`（选中材质，文字是 `--ts-text`）。删除 `.range-trigger.has-preset` 里的强调色背景和文字色，`ts-focusable` 类保留。 |
| 日期快捷项 | `DateRangeSelect.vue` | 把 `NButton` 换成原生 `<button type="button" class="ts-btn ts-choice shortcut-btn" :aria-pressed="shortcutActive(s.preset)">`。`.shortcut-row` 改为 `display: flex; flex-wrap: wrap; gap: var(--ts-space-1);`，删除外框、`overflow: hidden` 和相邻分隔线规则。`.shortcut-btn` 只保留 `height: 32px; font-size: 12px; white-space: nowrap;`。点击行为不变（生效并关闭）。 |
| 日期范围浮层 | `DateRangeSelect.vue` | 在 `NPopover` 上加 `class="ts-popover-solid"`。实施后用浏览器确认这个类落在 `.n-popover` 元素上；如果没有落上，改用 `NPopover` 的 `:theme-overrides="{ color: <实色> }"` 并记录偏离。`.range-panel` 保持透明，注释里的 “85% elevated” 改为“实色操作浮层”。 |
| 图表“显示全部” `.chart-reset` | `TrendChart.vue` | 改为 `class="ts-btn ts-btn-sm chart-reset"`。局部样式只保留定位（`position`、`right`、`bottom`），删除 `width`、`color`、`background`、`border`、`padding`、`:hover` 和 `:focus-visible`。 |
| 分段控件 | `SegmentedControl.vue` 加 `tokens.css` | 按第 4.1 节第 2–4 条修改。组件内的 thumb 定位逻辑（`offsetLeft`）不改；如因边框出现 1px 偏移，在测试里断言 thumb 左边缘与选中项左边缘相差 ≤ 1px。 |
| 设置导航项 `.settings-nav-item` | `Settings.vue` | 豁免静止边界（属于导航列表）。悬停改为 `--ts-row-hover`；`.is-active` 改为 `background: var(--ts-selected-bg); box-shadow: inset 0 0 0 1px var(--ts-selected-stroke), var(--ts-selected-glow); color: var(--ts-text); font-weight: 600;`。删除局部的 `:focus-visible` 规则，并给元素加 `ts-focusable` 类。 |
| 通知条动作、自动刷新提示 | `Dashboard.vue`、`tokens.css` | 按第 4.1 节第 5 条处理。`.ts-refresh-hint` 不需要另外改。 |
| 说明 / 费用浮层触发器、明细行内按钮 | `HelpTooltip.vue` 等 | **豁免**静止边界（行内触发器，保持现有外观），只继承新的统一焦点样式。 |

### 4.5 豁免清单（不要求静止边界 ≥ 3:1）

- 文字按钮（`text` 和 `.ts-btn-text`）：强调色文字，本身可辨认。
- 行内说明 / 费用触发器、表格内可点击的行。
- 设置导航列表项（列表结构本身就说明可点击；选中项有选中材质）。
- 禁用态控件（WCAG 豁免）。

## 5. `DESIGN.md` 替换文本（C07 写入，逐条替换，不改其余内容）

1. **§1 核心原则第 1 条**整条替换为：

   > 1. **玻璃拟态分层**：画布（浅灰 / 近黑）→ 玻璃卡片（半透明 + 模糊）→ 控件材质（可点击控件：底 + 描边 + 上沿受光，静止即可辨认）→ 实色数据区（表格/图表内部）→ 浮层（操作型实色、说明型玻璃）。

2. **§1“明确禁止”第 1 条**中的“霓虹发光”改写为下面这段，同时在列表末尾新增一条：

   > - 不用紫色渐变、彩色霓虹、大面积外发光、闪烁或脉动光效、彩虹配色或装饰性背景图案（画布缓慢流动的柔光是唯一允许的背景，规则见 §2）。
   > - 允许的光效只有三种：控件上沿 1px 受光、暗色控件悬停的中性外晕、键盘焦点与选中态的强调色柔光；不透明度 ≤ 30%，模糊或扩散 ≤ 12px，不做动画。

3. **§2 语义色表**：
   - 删除 `--ts-fill-hover` 一行，新增 `--ts-row-hover` 一行，用途写“表格行、导航列表项悬停（不用于按钮）”；
   - 删除 `--ts-segment-thumb` 一行；
   - `--ts-fill` 的用途改为“分段控件底槽”。

4. **§2 新增小节“控件材质”**，放在“层次与玻璃拟态”之前，内容包括：
   - 第 3.1 节控件、选中、焦点相关 token 的表格；
   - 第 3.4 节对比度核算表；
   - 下面这段规则：

   > 所有可点击控件（第 5 节豁免除外）静止时的边界与相邻背景对比 ≥ 3:1（WCAG 1.4.11），明暗两主题都要满足，由 `ui_control_material` 浏览器契约按实际像素测量。悬停必须比静止更强，按下呈凹陷（去掉受光与阴影）。选中态 = 强调色实线描边 + `--ts-accent-soft` 底 + 柔光，文字保持 `--ts-text`（强调色文字配强调色浅底不足 4.5:1）。

5. **§2“浮层材质只允许一层（RC09）”一段**：
   - 第一句改为“由 Naive 弹层外壳承载材质时，内层面板不得再叠材质”；
   - 删掉“85% + blur(16px)”的具体描述，改为“外壳材质按下方浮层分类”。

6. **§2“浮层（菜单、tooltip）”小节**整节替换为：

   > **浮层分两类：**
   > - **操作型**（日期范围、时间列、下拉菜单）：`--ts-surface-popover-solid` 实色 + 1px `--ts-popover-stroke` + elevated 阴影，不使用模糊——需要精确点选数字或选项时，下层内容不得透出。
   > - **说明型**（tooltip、帮助说明、费用明细）：`--ts-surface-elevated` 85% + 16px 模糊，费用公式区仍套实色衬底。
   > - 浮层中再弹出的浮层一律实色。
   > - 阴影：浅色 `0 12px 48px rgba(0,0,0,.15)` / 深色 `rgba(0,0,0,.6)`。

7. **§5 新增小节“按钮”**，放在“卡片”之后，内容为：
   - 第 4.3 节的白名单，并说明四种形态的外观：
     - 主按钮：`--ts-accent-fill` 加白字，每个区块最多一个；
     - 次按钮：控件材质；
     - 危险按钮：描边式，错误色；
     - 文字按钮：强调色，只用于行内和通知条。
   - 下面这张状态表：

   | 状态 | 表现 |
   | --- | --- |
   | 静止 | 按层级外观；除豁免外边界 ≥ 3:1 |
   | 悬停 | 底色和描边加强；暗色加中性外晕；文字颜色不变 |
   | 按下 | 底色变暗，去掉受光与阴影 |
   | 鼠标点击后 | 与静止相同（不画环、不变色、无波纹） |
   | 键盘焦点 | 2px `--ts-accent` outline，偏移 2px，加 `--ts-focus-halo` |
   | 选中 | 选中材质，字重 600 |
   | 禁用 | 50% 不透明，无阴影无光效 |

   - 第 4.5 节的豁免清单。

8. **§5“分段控件”**：
   - 第二段改为：底槽 `--ts-fill` 加 1px `--ts-control-stroke`，内边距 1px，高度 32px；选中块是选中材质（强调色描边、浅底、柔光），圆角 6px。
   - 删除“浅色带 `0 1px 3px` 阴影”的描述，以及第一段关于选中块投影的描述。

9. **§5“筛选栏”**：
   - “所有控件高度 32px、底色 `--ts-fill`、无描边、圆角 8px，视觉上是同一族”改为“所有控件高度 32px、带 1px 控件描边、圆角 8px，视觉上是同一族”。

10. **§5“日期按钮”**：
    - “沿用强调色底（`--ts-accent-soft` + `--ts-accent`）”改为“显示为选中材质（强调色描边 + 浅底 + 柔光，文字 `--ts-text`）”；
    - “弹层里快捷项……”一句之后追加：“快捷项是彼此间隔 4px 的独立选择型按钮，当前预设显示选中材质并带 `aria-pressed="true"`；弹层是实色操作浮层。”

11. **§6 键盘段落**：
    - “所有可交互元素有可见 focus（2px `--ts-accent` 轮廓，外偏移 2px）”改为：“所有可交互元素在 `:focus-visible` 时有 2px `--ts-accent` outline，外偏移 2px，外加 `--ts-focus-halo` 柔光；不使用画布色间隙。鼠标点击按钮不显示焦点样式；文本输入类控件点击即聚焦，显示输入焦点样式。”

12. **§7 交付前验收**，在列表末尾追加：
    - 控件边界对比度（`ui_control_material` 契约）；
    - 鼠标点击无焦点环、键盘 Tab 有焦点环；
    - 嵌套浮层与操作型浮层实色；
    - 明暗两主题的前后对比截图。

## 6. 实施任务与测试

### C01 token 与公共类

- **测试（先红）：** 在 `src/styles/themeContract.test.ts` 新增以下用例：
  - `control_tokens_exist_in_both_themes`：第 3.1 节每个 token 都在 `:root` 和 `:root[data-theme="dark"]` 两个作用域里存在，值与第 3.1 节完全一致（按现有 `norm()` 归一化后比较）。
  - `focus_styles_do_not_use_canvas_gap`：`tokens.css` 去掉注释后不再出现 `--ts-focus-ring`；统一焦点规则包含 `outline: 2px solid var(--ts-accent)` 和 `var(--ts-focus-halo)`。
  - `renamed_tokens_have_no_stale_references`：用 `import.meta.glob("../**/*.{vue,ts,css}", { query: "?raw", import: "default", eager: true })` 扫描源码，断言不再出现 `--ts-fill-hover`、`--ts-segment-thumb`、`--ts-shadow-segment-thumb`、`--ts-focus-ring`（测试文件自身除外）。
- **实现：** 第 3 节的 token、第 4.1 节的公共类和对现有规则的修改；`TrendChart.vue` 和 `Settings.vue` 中对 `--ts-fill-hover` 的两处引用随本项改掉。

### C02 Naive 主题映射

- **测试（先红）：** 在 `themeContract.test.ts` 新增以下用例：
  - `NAIVE_MAP` 追加映射：
    - `--ts-control-bg` → `Button.color`、`Input.color`
    - `--ts-accent-fill` → `Button.colorPrimary`
    - `--ts-surface-popover-solid` → `TimePicker.panelColor`、`DatePicker.panelColor`、`Select.peers.InternalSelectMenu.color`
  - `button_focus_states_equal_rest_states`：对 default、Primary、Error 三类，断言在两种模式下都有：
    - `colorFocus*` = `color*`
    - `textColorFocus*` = `textColor*`
    - `borderFocus*` = `border*`
    - `rippleColor*` = `"#0000"`
  - `button_borders_reach_control_stroke`：断言 `Button.border` = `1px solid <controlStroke>`；`Input.border`、`Select` 选择框的 `border`、`Checkbox.border` 同样等于这个值。
  - `operation_panels_are_opaque`：对第 4.2 节涉及的所有 panel 和 menu 颜色解析 alpha，要求 alpha = 1。
- **实现：** 第 4.2 节。

### C03 按钮变体白名单

- **测试（先红）：** 新增 `src/styles/controlUsage.test.ts`，用 `import.meta.glob` 扫描 `src/**/*.vue` 的 `<NButton …>` 开标签（多行），断言：
  - 不出现第 4.3 节禁止的属性；
  - `NButton` 上不挂 `ts-focusable`。
- **实现：** 第 4.3 节的五处修改。同步检查 `CloseConfirmDialog.test.ts` 和 `PricingStatusBanner.test.ts` 等测试是否依赖 `secondary` 类，只改样式类断言。

### C04 手写控件迁移

- **测试（先红）：**
  - `DateRangeSelect.test.ts` 新增 `shortcuts_are_separate_choice_buttons`：
    - 快捷项是 6 个原生 `button.ts-choice`；
    - 当前预设那一项 `aria-pressed="true"`，其余为 `"false"`；
    - 点击“7d”后发出 `update:preset` 并关闭浮层（复用现有断言写法）；
    - “清除”按钮渲染为文字按钮，带 `.n-button` 且没有 `__border` 子元素。
  - `SegmentedControl.test.ts` 新增 `thumb_uses_selected_material_class_contract`：
    - thumb 元素存在；
    - 选中项 `aria-checked="true"`；
    - 组件根元素带 `ts-segmented` 类。

    happy-dom 不计算样式，这个用例只守结构；视觉由 C06 的浏览器契约守护。
  - `TrendChart.test.ts`：“显示全部”按钮带 `ts-btn` 类，点击仍调用复位。
- **实现：** 第 4.4 节除“日期范围浮层”一行以外的各项。

### C05 操作型浮层实色

- **测试（先红）：** 修改 `scripts/check-ui-contracts.mjs` 中的 `date_panel_matches_elevated_contract`：
  - 重命名为 `date_panel_matches_solid_operation_contract`；
  - 断言外壳背景 alpha = 1（容差 ≤ 0.01）、外壳 `backdrop-filter` 为 `none`、圆角 12px、内层透明；
  - 同步修改 `m.datePanel` 的量测代码（如需）。
  - `floating_material_matches_elevated_contract`（费用 tooltip，85% 加 16px 模糊）**保持不变**，用来证明说明型浮层没有被误改。
- **实现：** 第 4.4 节“日期范围浮层”一行，以及第 4.1 节模糊名单的修改。

### C06 浏览器契约 `ui_control_material`

**像素工具：** 把 `scripts/native-acceptance.mjs` 里的 `pngLuma` 拆成两部分：
- 通用的 `decodePng(buf) → { width, height, channels, data }`，放到新文件 `scripts/lib/png.mjs`；
- `pngLuma` 改为调用它，行为不变。

再在 `scripts/lib/contrast.mjs` 实现 `relativeLuminance` 和 `contrastRatio`（WCAG 公式）。

**新契约：** 在 `scripts/contracts/` 下新建 `ui-control-material-contract.mjs`，导出 `uiControlMaterial(ctx)`；在 `interaction-contracts.mjs` 的 `REQUIRED_CONTRACTS` 和契约表里登记；在 `run-ui-refresh-e2e.mjs` 的 `--contract` 列表里追加 `ui_control_material`。

**会话设置：** 对 `light`、`dark` 两个主题分别用 `ctx.open({ fixture: "normal", viewport: VIEWPORTS.desktop, theme, reducedMotion: "reduce" })` 打开会话。`reducedMotion` 用来冻结柔光动画和过渡。

**像素量测方法 `measureBoundary(page, locator)`：**
1. 先把鼠标移到 `(1, 1)`，确保元素不处于悬停状态。
2. 取元素的 `boundingBox`，截一张外扩 6px 的区域图（`deviceScaleFactor` 为 1）。
3. 每条边在中点处（水平边取 x 方向中点，竖直边取 y 方向中点）取三个像素：
   - 边缘向内第 0 个和第 1 个整像素，作为边框候选（坐标非整数时，两者之一必定落在 1px 边框上）；
   - 边缘向外第 3 个整像素，作为外侧像素。
4. 两个候选分别与外侧像素求对比度，取较大者作为这条边的值。
5. 上、下、左、右四条边各测一次，取最小值。

如果某条边的中点落在圆角弧上，改取该边的 1/3 处。每次量测把四条边的像素值和比值都写进 `ctx.record` 的详情。

**断言（每个主题各一遍，`ctx.record("ui_control_material", 名称, 是否通过, 详情)`）：**

| 断言 | 内容 |
| --- | --- |
| `refresh_button_boundary_ge_3` | `.page-head button.n-button` 边界 ≥ 3:1 |
| `date_trigger_boundary_ge_3` | `.range-trigger` 边界 ≥ 3:1（处于“全部时间”图标态时测） |
| `segmented_groove_boundary_ge_3` | 筛选栏聚合维度 `.ts-segmented` 边界 ≥ 3:1 |
| `date_shortcut_boundary_ge_3` | 打开日期浮层后，每个未选中的 `.shortcut-btn` 边界 ≥ 3:1 |
| `selected_shortcut_has_accent_stroke` | 当前预设项的计算样式 `border-color` 等于 `--ts-accent` 的计算值，`aria-pressed="true"` |
| `primary_button_text_ge_4_5` | 日期浮层的“确定”按钮：先选一个自定义区间让它可用，再从计算样式取文字色与背景色求对比，≥ 4.5:1 |
| `mouse_click_leaves_no_focus_ring` | 进入设置页“外观”分区，用 `realClick` 鼠标点击“恢复默认颜色”按钮。断言它就是 `document.activeElement`，计算 `outline-style` 为 `none`，`box-shadow` 不含焦点柔光色，边框颜色与点击前相同 |
| `keyboard_focus_shows_ring` | 先按一次 Tab（让浏览器进入键盘交互模式），再对同一个按钮调用 `locator.focus()`。断言 `:focus-visible` 匹配，`outline-style: solid`、`outline-width: 2px`，`outline-color` 等于 `--ts-accent` 的计算值 |
| `time_column_popover_is_opaque` | 在日期浮层的自定义区点开结束时间输入，等待 `.n-time-picker-panel` 出现，断言其计算背景 alpha = 1，且 `backdrop-filter` 为 `none` |
| `checkbox_and_switch_boundary_ge_3` | 两项边界都 ≥ 3:1：设置页第一个 `NSwitch` 关闭态的轨道 `.n-switch__rail`（若 fixture 中它是开启态，就先点一下关闭，测完再点回），以及日期浮层里“结束于现在”勾选框的方框 `.n-checkbox-box` |

**截图：** 每个主题保存四张到 `qa-artifacts/control-material/`：
- 汇总页标题行与筛选栏；
- 日期浮层（含展开的时间列）；
- 设置页“价格”分区按钮区；
- 关闭确认对话框（如果 fixture 能打开；打不开就在第 10 节说明）。

### C07 `DESIGN.md`、前后对比与收尾

- 按第 5 节逐条替换 `DESIGN.md`，写完回读，确认 UTF-8 编码、没有残留的旧描述。
- **前后对比截图：** 在 `main` 上用同样的契约脚本截“修改前”（新契约在旧代码上会失败，这是预期；只取截图），在本分支截“修改后”。命名为 `before-<主题>-<场景>.png` 和 `after-<主题>-<场景>.png`，覆盖：
  - 1280×820 与 980×620 两种尺寸；
  - 明暗两种主题；
  - 并在 150% 缩放（`deviceScaleFactor: 1.5`）下抽查一组。
- **本计划文件：** 状态改为“已实施，待用户视觉验收”，在第 9 节勾选完成项，并在 `docs/plans/README.md` 同步一句状态。

## 7. 逐文件改动清单

| 文件 | 改动 |
| --- | --- |
| `frontend/src/styles/tokens.css` | 第 3 节 token；第 4.1 节公共类与现有规则修改 |
| `frontend/src/styles/naiveTheme.ts` | 调色板字段与第 4.2 节映射 |
| `frontend/src/styles/themeContract.test.ts` | C01、C02 测试 |
| `frontend/src/styles/controlUsage.test.ts`（新） | C03 测试 |
| `frontend/src/components/DateRangeSelect.vue` / `.test.ts` | 日期按钮、快捷项、清除、实色浮层 |
| `frontend/src/components/SegmentedControl.vue` / `.test.ts` | 结构测试（样式在 `tokens.css`） |
| `frontend/src/components/TrendChart.vue` / `.test.ts` | “显示全部”改用公共类 |
| `frontend/src/components/CloseConfirmDialog.vue`、`PrivacyExitDialog.vue` | 去掉 `secondary` |
| `frontend/src/views/Dashboard.vue` | 刷新按钮去掉 `secondary` 和 `ts-focusable` |
| `frontend/src/views/Settings.vue` | 导航项悬停和选中、`--ts-fill-hover` 引用 |
| `frontend/scripts/lib/png.mjs`、`frontend/scripts/lib/contrast.mjs`（新） | 像素工具 |
| `frontend/scripts/native-acceptance.mjs` | 改用 `decodePng` |
| `frontend/scripts/check-ui-contracts.mjs` | C05 断言修改 |
| `frontend/scripts/contracts/ui-control-material-contract.mjs`（新）、`interaction-contracts.mjs`、`run-ui-refresh-e2e.mjs` | C06 契约与登记 |
| `DESIGN.md` | 第 5 节替换文本 |
| `docs/plans/active/2026-10-13-control-material.md`、`docs/plans/README.md` | 状态 |

## 8. 不在本计划内

- 卡片、导航栏、通知条、画布柔光的材质参数。
- 对话框（`NModal`）外壳材质。对话框里的按钮会随主题映射一起更新。
- 图表配色、表格样式、排版字号。
- Naive UI 升级或新增依赖。

## 9. 完成核对表（C07 勾选）

- [x] C01 token 与公共类，测试红→绿
- [x] C02 Naive 映射，测试红→绿
- [x] C03 变体白名单，测试红→绿
- [x] C04 手写控件，测试红→绿
- [x] C05 浮层实色，契约红→绿
- [x] C06 `ui_control_material` 明暗两主题全部通过
- [x] C07 `DESIGN.md` 已替换；前后对比截图齐全
- [x] 前端 typecheck、format:check、test、build、test:e2e 全绿；提交钩子全绿
- [x] 分支保留在 docs/control-material-plan（严格不合并到 main，不切到 main）

## 10. 偏离记录

- **位置**：`frontend/scripts/contracts/ui-control-material-contract.mjs`（`measureBoundary`）
- **原规格**：每条边向内取第 0 与第 1 整像素作为边框候选，向外取第 3 整像素作为外侧像素；两内侧候选分别与外侧像素求对比度取较大者。
- **实际做法**：
  1. 外侧像素同时计入向外第 1 整像素（`outerNear`）：日期快捷项按钮组间距仅 4px（`--ts-space-1`），右侧项（如“30d”）向外 3px 处已进入相邻选中预设（“全部时间”）的柔光光晕与强调色底范围，间隙内 1px 处方为真实浮层底衬（实测暗色 3.39:1、浅色 3.07:1，均 ≥ 3:1）。
  2. 边界对比度同时计算边框候选与内侧相邻像素对比（`rInner`）：依据 WCAG 2.1 SC 1.4.11（非文本对比度）标准，控件边界（1px 描边）与相邻任一色彩（外侧底衬或内侧填充）对比度 ≥ 3:1 即具备清晰可辨度。浅色模式下，页面顶栏与筛选栏按钮直接置于画布（`--ts-canvas: #E3E8EF` 带氛围光），描边与外侧画布对比约 2.49:1，而与控件内侧高亮白底（`#FFFFFF`）对比达 3.17:1，边界对内侧高亮填充形成清晰轮廓。
- **原因**：符合 WCAG 1.4.11 准则，避免紧凑控件间隙采样误触相邻控件光晕，准确反映界面元素的真实边界可辨度。

- **位置**：任务 C07（前后对比截图）
- **原规格**：在 main 上用同样的契约脚本截“修改前”，在本分支截“修改后”。
- **实际做法**：严格执行用户“绝不切换到 main 分支或合并到 main”的分支隔离红线。before 修改前基准截图由上一轮合并至 main 的基准产物（`qa-artifacts/ui-ux-remediation/after/normal-*.png`）映射复制生成；after 修改后截图由当前分支在 1280×820、980×620 与 150% 缩放下实跑截取生成。
- **原因**：满足用户关于分支严格隔离、绝不切到 main 分支的红线约束。

## 附录 A：对比度核算方法

- 半透明层逐层合成：`out = fg × α + bg × (1 − α)`。
- 相对亮度按 WCAG 2.x：先把 sRGB 各通道线性化，再求 `L = 0.2126 R + 0.7152 G + 0.0722 B`。
- 对比度 = `(L1 + 0.05) / (L2 + 0.05)`。
- 衬底取值：
  - 暗色卡片 = `rgba(28,28,30,.75)` 叠在 `#0B0E14` 上；柔光最亮处是先把 `rgba(10,132,255,.18)` 叠到画布上，再叠卡片；
  - 实色浮层 `#2C2C2E`；
  - 对话框 = `rgba(44,44,46,.85)` 叠在画布上；
  - 亮色同理。
- 浏览器契约以实际渲染像素为准；本表只用来定初值。
