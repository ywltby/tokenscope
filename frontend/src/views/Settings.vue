<script setup lang="ts">
import { computed, h, onMounted, ref, watch, type VNode } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  NAlert,
  NButton,
  NCard,
  NDataTable,
  NGrid,
  NGi,
  NSelect,
  NSpin,
  NStatistic,
  NInput,
  NSwitch,
  NTag,
  NTooltip,
  useMessage,
  type DataTableColumn,
} from "naive-ui";
import {
  AGENT_LABEL,
  fmtNum,
  type CacheInfo,
  type PricingEntry,
  type PricingView,
  type SourceStatus,
} from "../types";
import { TZ_OPTIONS, useTimezone } from "../composables/timezone";

const props = defineProps<{ refreshKey: number }>();
const msg = useMessage();

const sources = ref<SourceStatus[]>([]);
const cache = ref<CacheInfo | null>(null);
const pricing = ref<PricingView | null>(null);
const syncing = ref(false);
const loading = ref(false);
const rebuilding = ref(false);
const { tz } = useTimezone();
const autostart = ref<boolean | null>(null);
const autostartBusy = ref(false);
const autoSync = ref<boolean | null>(null);
const autoSyncBusy = ref(false);

async function loadAll(): Promise<void> {
  loading.value = true;
  try {
    sources.value = await invoke<SourceStatus[]>("source_status");
    cache.value = await invoke<CacheInfo>("cache_stats");
    pricing.value = await invoke<PricingView>("pricing_entries");
    await loadDrafts();
  } finally {
    loading.value = false;
  }
}

// C1：来源配置草稿（编辑后按行保存）
const drafts = ref<Record<string, { enabled: boolean; dir: string }>>({});
const savingSource = ref<string | null>(null);

async function loadDrafts(): Promise<void> {
  const s = await invoke<Record<string, unknown>>("settings_get");
  const src = (s.sources ?? {}) as Record<string, { enabled?: boolean; dir?: string | null }>;
  drafts.value = {
    claude: { enabled: src.claude?.enabled ?? true, dir: src.claude?.dir ?? "" },
    codex: { enabled: src.codex?.enabled ?? true, dir: src.codex?.dir ?? "" },
  };
}

async function saveSource(agent: string): Promise<void> {
  savingSource.value = agent;
  try {
    const d = drafts.value[agent];
    await invoke("source_config_set", {
      agent,
      enabled: d.enabled,
      dir: d.dir.trim() === "" ? null : d.dir.trim(),
    });
    await loadSources();
  } finally {
    savingSource.value = null;
  }
}

async function loadSources(): Promise<void> {
  sources.value = await invoke<SourceStatus[]>("source_status");
}

async function rebuild(): Promise<void> {
  rebuilding.value = true;
  try {
    cache.value = await invoke<CacheInfo>("refresh_cache");
    msg.success("缓存已重建");
  } catch (e) {
    msg.error(String(e));
  } finally {
    rebuilding.value = false;
  }
}

async function syncPricing(): Promise<void> {
  syncing.value = true;
  try {
    const reports = await invoke<{ source: string; count: number }[]>("sync_pricing_openrouter");
    msg.success(reports.map((r) => `${r.source} ${r.count} 条`).join("，"));
    pricing.value = await invoke<PricingView>("pricing_entries");
  } catch (e) {
    msg.error(`同步失败：${e}`);
  } finally {
    syncing.value = false;
  }
}

async function openPricing(): Promise<void> {
  try {
    const p = await invoke<string>("open_pricing_file");
    msg.info(`已在系统默认编辑器打开（保存后下次统计生效）\n${p}`);
    pricing.value = await invoke<PricingView>("pricing_entries");
  } catch (e) {
    msg.error(String(e));
  }
}

async function loadAutostart(): Promise<void> {
  autostart.value = await invoke<boolean>("autostart_status");
}

async function setAutostart(enabled: boolean): Promise<void> {
  autostartBusy.value = true;
  try {
    autostart.value = await invoke<boolean>("autostart_set", { enabled });
    msg.success(enabled ? "已开启开机自启" : "已关闭开机自启");
  } catch (e) {
    msg.error(String(e));
  } finally {
    autostartBusy.value = false;
  }
}

async function loadAutoSync(): Promise<void> {
  const settings = await invoke<{ price_auto_sync: boolean }>("settings_get");
  autoSync.value = settings.price_auto_sync;
}

async function setAutoSync(enabled: boolean): Promise<void> {
  autoSyncBusy.value = true;
  try {
    autoSync.value = await invoke<boolean>("settings_set_price_auto_sync", { enabled });
    msg.success(enabled ? "已开启自动同步（每 24h）" : "已关闭自动同步");
  } catch (e) {
    msg.error(String(e));
  } finally {
    autoSyncBusy.value = false;
  }
}

onMounted(() => {
  void loadAll();
  void loadAutostart();
  void loadAutoSync();
});
watch(
  () => props.refreshKey,
  () => void loadAll(),
);

/// 单价悬浮提示：来源 + OpenRouter 同前缀对照价（无对应模型标注未知价格）。
function priceCell(r: object, pick: (e: PricingEntry) => number): VNode {
  const e = asEntry(r);
  const or = e.openrouter;
  const orLine = or
    ? `OpenRouter：输入 ${fmtPrice(or.input)} · 输出 ${fmtPrice(or.output)} · 缓存写 ${fmtPrice(or.cache_write)} · 缓存读 ${fmtPrice(or.cache_read)}`
    : "OpenRouter：未知价格（无对应模型）";
  return h(NTooltip, null, {
    trigger: () => h("span", { style: "cursor: help" }, fmtPrice(pick(e))),
    default: () =>
      h("div", { style: "font-size: 12px; line-height: 1.8" }, [
        h(
          "div",
          `来源：${e.source}${e.incomplete ? "（不完整：部分分项价格未知，按 0 展示但未计费）" : ""}`,
        ),
        h("div", orLine),
        or?.name ? h("div", { style: "opacity: 0.7" }, `模型：${or.name}`) : null,
      ]),
  });
}

function fmtPrice(v: number): string {
  if (v === 0) return "0";
  if (v < 0.001) return v.toFixed(6);
  if (v < 1) return v.toFixed(4);
  return v.toFixed(2);
}

const priceColumns = computed<DataTableColumn[]>(() => [
  {
    title: "显示名",
    key: "name",
    minWidth: 180,
    ellipsis: { tooltip: true },
    render: (r) => asEntry(r).name ?? "",
  },
  { title: "模型前缀", key: "prefix", minWidth: 220 },
  { title: "输入$", key: "input", align: "right", render: (r) => priceCell(r, (e) => e.input) },
  { title: "输出$", key: "output", align: "right", render: (r) => priceCell(r, (e) => e.output) },
  {
    title: "缓存写$",
    key: "cache_write",
    align: "right",
    render: (r) => priceCell(r, (e) => e.cache_write),
  },
  {
    title: "缓存读$",
    key: "cache_read",
    align: "right",
    render: (r) => priceCell(r, (e) => e.cache_read),
  },
  {
    title: "来源",
    key: "source",
    render: (r) =>
      h(
        NTag,
        {
          size: "small",
          bordered: false,
          type:
            asEntry(r).source === "外置"
              ? "success"
              : asEntry(r).source === "openrouter"
                ? "info"
                : "default",
        },
        { default: () => asEntry(r).source },
      ),
  },
]);

/// Naive UI 表格行类型是 Record<string, unknown>，统一经 unknown 转换。
const asEntry = (r: object): PricingEntry => r as unknown as PricingEntry;
// 同前缀可能同时存在内置/openrouter/外置行，键必须含来源
const rowKey = (r: object): string => `${asEntry(r).source}|${asEntry(r).prefix}`;
</script>

<template>
  <NSpin :show="loading">
    <!-- 最小高度保证加载转圈居中于可视区 -->
    <div style="min-height: 380px">
      <NGrid :cols="2" :x-gap="12" :y-gap="12" item-responsive responsive="screen">
        <NGi span="1">
          <NCard title="桌面体验" size="small">
            <div style="margin-bottom: 12px">
              <NStatistic label="聚合/展示时区" :value="tz === 'local' ? '本机时区' : tz" />
              <div style="font-size: 12px; opacity: 0.6; margin: 4px 0 8px">
                存储/计算一律 UTC，仅展示按此时区一次转换。
              </div>
              <NSelect v-model:value="tz" :options="TZ_OPTIONS" size="small" style="width: 200px" />
            </div>
            <div>
              <NStatistic
                label="开机自启"
                :value="autostart == null ? '—' : autostart ? '已开启' : '已关闭'"
              />
              <div style="font-size: 12px; opacity: 0.6; margin: 4px 0 8px">
                开机后自动启动并驻留托盘。
              </div>
              <NSwitch
                :value="autostart === true"
                :disabled="autostart == null || autostartBusy"
                :loading="autostartBusy"
                @update:value="setAutostart"
              />
            </div>
          </NCard>
        </NGi>
        <NGi span="1">
          <NCard title="数据来源" size="small">
            <div v-for="s in sources" :key="s.agent" style="margin-bottom: 16px">
              <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 6px">
                <strong>{{ AGENT_LABEL[s.agent] ?? s.agent }}</strong>
                <NSwitch
                  :value="drafts[s.agent]?.enabled ?? true"
                  size="small"
                  @update:value="(v: boolean) => (drafts[s.agent]!.enabled = v)"
                />
                <span style="font-size: 12px; opacity: 0.6">
                  {{
                    s.state === "ready"
                      ? `${fmtNum(s.files)} 个会话文件`
                      : s.state === "disabled"
                        ? "已停用"
                        : s.state === "missing"
                          ? "目录不存在"
                          : "无日志"
                  }}
                </span>
                <span style="flex: 1"></span>
                <NButton
                  size="tiny"
                  :loading="savingSource === s.agent"
                  @click="saveSource(s.agent)"
                >
                  保存
                </NButton>
              </div>
              <NInput
                :value="drafts[s.agent]?.dir ?? ''"
                size="small"
                :placeholder="`默认目录：${s.dir}`"
                @update:value="(v: string) => (drafts[s.agent]!.dir = v)"
              />
              <div style="font-size: 12px; opacity: 0.6; margin-top: 4px">
                留空使用默认目录；停用后该来源完全不参与统计。保存后回到汇总页生效。
              </div>
            </div>
          </NCard>
        </NGi>
        <NGi span="1">
          <NCard title="解析缓存" size="small">
            <NStatistic label="缓存文件" :value="cache ? fmtNum(cache.files) : '—'" />
            <NStatistic label="缓存事件" :value="cache ? fmtNum(cache.events) : '—'" />
            <div style="font-size: 12px; opacity: 0.6; margin: 6px 0">{{ cache?.path }}</div>
            <div style="font-size: 12px; opacity: 0.6; margin-bottom: 10px">
              缓存是纯优化：任何故障都会自动退回全量扫描，统计数字不受影响。
            </div>
            <NButton size="small" :loading="rebuilding" @click="rebuild">重建缓存</NButton>
          </NCard>
        </NGi>
        <NGi span="2">
          <NCard title="模型价格表" size="small">
            <template #header-extra>
              <div style="display: flex; gap: 8px">
                <NButton size="small" type="primary" :loading="syncing" @click="syncPricing">
                  同步在线价格
                </NButton>
                <NButton size="small" @click="openPricing">打开 / 创建外置价格文件</NButton>
              </div>
            </template>
            <div style="font-size: 12px; opacity: 0.6; margin-bottom: 8px">
              优先级：外置（{{ pricing?.external_count ?? 0 }} 条）> models.dev（{{
                pricing?.modelsdev_count ?? 0
              }}
              条）> OpenRouter（{{ pricing?.openrouter_count ?? 0 }} 条）> 内置；层内最长前缀匹配。
              <template v-if="pricing?.modelsdev_synced_at">
                models.dev 上次同步：{{ pricing.modelsdev_synced_at }}；</template
              >
              <template v-if="pricing?.openrouter_synced_at"
                >OpenRouter 上次同步：{{ pricing.openrouter_synced_at }}</template
              >
              <template v-if="!pricing?.modelsdev_synced_at && !pricing?.openrouter_synced_at"
                >尚未同步在线源（同步前仅内置 + 外置生效）。</template
              >
            </div>
            <NAlert
              v-for="(w, i) in pricing?.warnings ?? []"
              :key="i"
              type="warning"
              style="margin-bottom: 8px"
            >
              {{ w }}
            </NAlert>
            <div
              style="
                display: flex;
                align-items: center;
                justify-content: space-between;
                gap: 12px;
                margin-bottom: 8px;
              "
            >
              <span style="font-size: 13px; opacity: 0.8">自动同步价格（每 24h）</span>
              <NSwitch
                :value="autoSync === true"
                :disabled="autoSync == null || autoSyncBusy"
                :loading="autoSyncBusy"
                @update:value="setAutoSync"
              />
            </div>
            <div style="font-size: 12px; opacity: 0.6; margin-bottom: 8px">
              {{ pricing?.path }}（TOML；本地价格最高优先，保存后下次统计生效）
            </div>
            <NDataTable
              :columns="priceColumns"
              :data="pricing?.entries ?? []"
              :row-key="rowKey"
              size="small"
              :max-height="420"
              virtual-scroll
            />
          </NCard>
        </NGi>
      </NGrid>
    </div>
  </NSpin>
</template>
