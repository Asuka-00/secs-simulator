/**
 * App i18n (zh-CN / en-US).
 * 应用多语言（中文 / 英文）。
 *
 * Locale is persisted in localStorage under `secs-sim-locale`.
 * 语言选择持久化到 localStorage 键 `secs-sim-locale`。
 */

import { createI18n } from "vue-i18n";
import enUS from "./locales/en-US";
import zhCN from "./locales/zh-CN";

export type AppLocale = "zh-CN" | "en-US";

export const LOCALE_KEY = "secs-sim-locale";

export function detectLocale(): AppLocale {
  try {
    const saved = localStorage.getItem(LOCALE_KEY);
    if (saved === "zh-CN" || saved === "en-US") return saved;
  } catch {
    /* ignore */
  }
  const nav = typeof navigator !== "undefined" ? navigator.language : "";
  if (nav.toLowerCase().startsWith("zh")) return "zh-CN";
  return "en-US";
}

export const i18n = createI18n({
  legacy: false,
  locale: detectLocale(),
  fallbackLocale: "en-US",
  messages: {
    "en-US": enUS,
    "zh-CN": zhCN,
  },
});

export function setAppLocale(locale: AppLocale) {
  i18n.global.locale.value = locale;
  try {
    localStorage.setItem(LOCALE_KEY, locale);
  } catch {
    /* ignore */
  }
  document.documentElement.lang = locale === "zh-CN" ? "zh-CN" : "en";
}

// Apply initial html lang / 设置初始 html lang
setAppLocale(detectLocale());
