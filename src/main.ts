/**
 * Application entry.
 * 应用入口。
 */

import { createApp } from "vue";
import { createPinia } from "pinia";
import ElementPlus from "element-plus";
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import App from "./App.vue";
import { i18n } from "./i18n";
import "./theme"; // apply theme before paint / 渲染前应用主题
import "./styles.css";

const app = createApp(App);
app.use(createPinia());
app.use(i18n);
app.use(ElementPlus);
app.mount("#app");
