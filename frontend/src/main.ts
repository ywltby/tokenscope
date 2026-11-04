import { createApp } from "vue";
import "./styles/tokens.css";
import naive from "naive-ui";
import App from "./App.vue";
import { i18n } from "./lib/i18n";

// i18n 在引导壳创建时就安装：隐私政策弹窗与检查提示同样需要双语，
// 同意前只使用内存态语言（默认 zh-CN），不读写任何持久化偏好。
createApp(App).use(naive).use(i18n).mount("#app");
