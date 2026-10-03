<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  NAlert,
  NButton,
  NCard,
  NDataTable,
  NGrid,
  NGi,
  NSpin,
  NStatistic,
  NTag,
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

const props = defineProps<{ refreshKey: number }>();
const msg = useMessage();

const sources = ref<SourceStatus[]>([]);
const cache = ref<CacheInfo | null>(null);
const pricing = ref<PricingView | null>(null);
const loading = ref(false);
const rebuilding = ref(false);

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

async function openPricing(): Promise<void> {
  try {
    const p = await invoke<string>("open_pricing_file");
    msg.info(`已在系统默认编辑器打开（保存后下次统计生效）\n${p}`);
    pricing.value = await invoke<PricingView>("pricing_entries");
  } catch (e) {
    msg.error(String(e));
  }
}

onMounted(loadAll);
watch(
  () => props.refreshKey,
  () => void loadAll(),
);

const priceColumns = computed<DataTableColumn[]>(() => [
  { title: "模型前缀", key: "prefix", minWidth: 220 },
  {
    title: "输入$",
    key: "input",
    align: "right",
    render: (r) => asEntry(r).input.toFixed(3),
  },
  {
    title: "输出$",
    key: "output",
    align: "right",
    render: (r) => asEntry(r).output.toFixed(3),
  },
  {
    title: "缓存写$",
    key: "cache_write",
    align: "right",
    render: (r) => asEntry(r).cache_write.toFixed(3),
  },
  {
    title: "缓存读$",
    key: "cache_read",
    align: "right",
    render: (r) => asEntry(r).cache_read.toFixed(3),
  },
  {
    title: "来源",
    key: "source",
    render: (r) =>
      h(
        NTag,
        { size: "small", bordered: false, type: asEntry(r).source === "外置" ? "success" : "default" },
        { default: () => asEntry(r).source },
      ),
  },
]);

/// Naive UI 表格行类型是 Record<string, unknown>，统一经 unknown 转换。
const asEntry = (r: object): PricingEntry => r as unknown as PricingEntry;
const rowKey = (r: object): string => asEntry(r).prefix;
</script>

<template>
  <NSpin :show="loading">
    <NGrid :cols="2" :x-gap="12" :y-gap="12" item-responsive responsive="screen">
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
            <NButton size="small" @click="openPricing">打开 / 创建外置价格文件</NButton>
          </template>
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
  </NSpin>
</template>
