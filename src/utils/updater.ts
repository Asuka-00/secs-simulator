import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateCheck =
  | { kind: "none" }
  | { kind: "available"; version: string; notes: string; install: () => Promise<void> };

export async function checkAppUpdate(): Promise<UpdateCheck> {
  const update = await check();
  if (!update) return { kind: "none" };
  return {
    kind: "available",
    version: update.version,
    notes: update.body ?? "",
    install: async () => {
      await update.downloadAndInstall();
      await relaunch();
    },
  };
}
