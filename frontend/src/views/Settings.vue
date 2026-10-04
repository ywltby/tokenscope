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
const loading = ref(false);
const rebuilding = ref(false);
const { tz } = useTimezone();
const autostart = ref<boolean | null>(null);
const autostartBusy = ref(false);
const syncing = ref(false);

async function loadAll(): Promise<void> {
  loading.value = true;
  try {
    sources.value = await invoke<SourceStatus[]>("source_status");
    cache.value = await invoke<CacheInfo>("cache_stats");
    pricing.value = await invoke<PricingView>("pricing_entries");
  } finally {
    loading.value = false;
  }
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

async function syncOpenRouter(): Promise<void> {
  syncing.value = true;
  try {
    const r = await invoke<{ count: number; synced_at: string }>("sync_pricing_openrouter");
    msg.success(`已同步 ${r.count} 个模型价格`);
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

onMounted(() => {
  void loadAll();
  void loadAutostart();
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
        h("div", `来源：${e.source}`),
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
            <div v-for="s in sources" :key="s.agent" style="margin-bottom: 12px">
              <NStatistic
                :label="AGENT_LABEL[s.agent] ?? s.agent"
                :value="s.exists ? `${fmtNum(s.files)} 个会话文件` : '未安装'"
              />
              <div style="font-size: 12px; opacity: 0.6">{{ s.dir }}</div>
              <NAlert v-if="!s.exists" type="warning" style="margin-top: 6px">
                目录不存在，该来源将没有统计。
              </NAlert>
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
                <NButton size="small" type="primary" :loading="syncing" @click="syncOpenRouter">
                  同步 OpenRouter 价格
                </NButton>
                <NButton size="small" @click="openPricing">打开 / 创建外置价格文件</NButton>
              </div>
            </template>
            <div style="font-size: 12px; opacity: 0.6; margin-bottom: 8px">
              优先级：外置（{{ pricing?.external_count ?? 0 }} 条）> OpenRouter（{{
                pricing?.openrouter_count ?? 0
              }}
              条）> 内置；层内最长前缀匹配。
              <template v-if="pricing?.synced_at"
                >OpenRouter 上次同步：{{ pricing.synced_at }}</template
              >
              <template v-else>尚未同步 OpenRouter（同步前仅内置 + 外置生效）。</template>
            </div>
            <NAlert
              v-for="(w, i) in pricing?.warnings ?? []"
              :key="i"
              type="warning"
              style="margin-bottom: 8px"
            >
              {{ w }}
            </NAlert>
            <div style="font-size: 12px; opacity: 0.6; margin-bottom: 8px">
              {{ pricing?.path }}（TOML；同前缀覆盖内置，最长前缀匹配；保存后下次统计生效）
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
