import { computed, readonly, ref } from "vue";
import { TOKEN_BUCKETS, type TokenBucketKey } from "../lib/tokenDisplay";
import { readItem, writeItem } from "../lib/localStorage";
import { chartTokens } from "../styles/chartTheme";
import { useTheme } from "./theme";

const STORAGE_KEY = "tokenscope-token-colors";
type Overrides = Partial<Record<TokenBucketKey, string>>;
const validColor = (value: unknown): value is string =>
  typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value);

function initialColors(): Overrides {
  try {
    const saved: unknown = JSON.parse(readItem(STORAGE_KEY) ?? "{}");
    if (!saved || typeof saved !== "object" || Array.isArray(saved)) return {};
    const result: Overrides = {};
    for (const { key } of TOKEN_BUCKETS) {
      const value = (saved as Record<string, unknown>)[key];
      if (validColor(value)) result[key] = value.toUpperCase();
    }
    return result;
  } catch {
    return {};
  }
}

const overrides = ref<Overrides>(initialColors());
const persistenceError = ref(false);
function save(next: Overrides): void {
  overrides.value = next;
  persistenceError.value = !writeItem(STORAGE_KEY, JSON.stringify(next));
}

export function useTokenColors() {
  const { mode } = useTheme();
  const colors = computed(
    () =>
      Object.fromEntries(
        chartTokens(mode.value, overrides.value).series.map((s) => [s.key, s.color]),
      ) as Record<TokenBucketKey, string>,
  );
  return {
    colors,
    overrides: readonly(overrides),
    persistenceError: readonly(persistenceError),
    setColor(key: TokenBucketKey, value: string): boolean {
      if (!validColor(value)) return false;
      save({ ...overrides.value, [key]: value.toUpperCase() });
      return true;
    },
    resetColors() {
      save({});
    },
  };
}
