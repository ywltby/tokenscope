// 主界面动态加载器（BS02）：主界面 import 失败时给出可见、可操作的错误，
// 不再无提示白屏。失败后只能整页重新加载——同一文档里失败的模块请求会被
// 模块映射缓存，再次 import() 不会恢复。
import { ref, type Ref } from "vue";

export interface MainAppLoaderOptions {
  /** 动态导入主界面模块（注入以便测试）。 */
  load: () => Promise<unknown>;
  /** 主界面是否仍应挂载（导入期间就绪可能已丢失，例如进入退出流程）。 */
  isReady: () => boolean;
}

export interface MainAppLoader {
  /** 导入失败的可展示原因（null = 没有失败）。 */
  error: Ref<string | null>;
  /** 主界面已成功导入且就绪仍成立。 */
  visible: Ref<boolean>;
  /** 进入主应用：并发调用单飞，共用同一次加载。 */
  enter: () => Promise<void>;
}

function describe(e: unknown): string {
  if (e instanceof Error) return e.message;
  return typeof e === "string" ? e : String(e);
}

export function createMainAppLoader({ load, isReady }: MainAppLoaderOptions): MainAppLoader {
  const error = ref<string | null>(null);
  const visible = ref(false);
  let inFlight: Promise<void> | null = null;

  async function enter(): Promise<void> {
    if (visible.value) return;
    if (inFlight) return inFlight;
    const run = async (): Promise<void> => {
      try {
        await load();
      } catch (e) {
        error.value = describe(e);
        console.error(`主界面加载失败：${error.value}`);
        return;
      }
      // 导入期间可能已进入退出流程：丢失的就绪不得复活主应用。
      if (!isReady()) return;
      visible.value = true;
    };
    inFlight = run().finally(() => {
      inFlight = null;
    });
    return inFlight;
  }

  return { error: error as Ref<string | null>, visible: visible as Ref<boolean>, enter };
}
