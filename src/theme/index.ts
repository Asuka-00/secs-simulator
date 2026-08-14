/**
 * Light / dark theme switch (persisted).
 * 明暗主题切换（持久化）。
 */

export type AppTheme = "light" | "dark";

export const THEME_KEY = "secs-sim-theme";

export function detectTheme(): AppTheme {
  try {
    const saved = localStorage.getItem(THEME_KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    /* ignore */
  }
  if (typeof window !== "undefined" && window.matchMedia) {
    if (window.matchMedia("(prefers-color-scheme: light)").matches) {
      return "light";
    }
  }
  return "dark";
}

/** Apply theme class on <html> / 在 html 上应用主题 class */
export function applyTheme(theme: AppTheme) {
  const root = document.documentElement;
  root.classList.toggle("dark", theme === "dark");
  root.classList.toggle("light", theme === "light");
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

export function setAppTheme(theme: AppTheme) {
  applyTheme(theme);
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    /* ignore */
  }
}

export function toggleAppTheme(): AppTheme {
  const next: AppTheme = document.documentElement.classList.contains("dark")
    ? "light"
    : "dark";
  setAppTheme(next);
  return next;
}

export function getAppTheme(): AppTheme {
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

// Apply as early as possible (imported from main) / 尽早应用（由 main 引入）
applyTheme(detectTheme());
