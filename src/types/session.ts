/**
 * Mirror of Rust session DTOs (serde camelCase).
 * 与 Rust 会话 DTO 镜像（serde camelCase）。
 */

export type Role = "host" | "equipment";
export type ConnectionMode = "active" | "passive";
export type ClockType = "a12" | "a16";

export interface SessionConfig {
  name: string;
  role: Role;
  mode: ConnectionMode;
  ip: string;
  port: number;
  sessionId: number;
  t3: number;
  t5: number;
  t6: number;
  t7: number;
  t8: number;
  linktestEnabled: boolean;
  linktestSeconds: number;
  rebindIfPassive: boolean;
  mdln: string;
  softrev: string;
  clockType: ClockType;
}

export type LogDirection = "tx" | "rx" | "passThrough" | "system";

export interface LogEntry {
  id: string;
  timestampMs: number;
  direction: LogDirection;
  stream?: number | null;
  function?: number | null;
  wbit?: boolean | null;
  systemBytes?: number | null;
  session?: number | null;
  summary: string;
  sml?: string | null;
  hex?: string | null;
}

export interface SessionSummary {
  id: string;
  name: string;
  open: boolean;
  hsmsState: string;
}

/** Structured SECS-II body node (tree editor / SMD ItemName). */
export interface BodyItem {
  id: string;
  type: string;
  name: string;
  value: string;
  children: BodyItem[];
}

/** Prefabricated SECS message (from SMD). */
export interface PrefabMessage {
  id: string;
  messageName: string;
  description: string;
  pairName: string;
  stream: number;
  function: number;
  direction: string;
  wait: boolean;
  autoReply: boolean;
  noLogging: boolean;
  bodySml: string;
  /** Preferred structured body (ItemName preserved from SMD). */
  bodyTree?: BodyItem[];
}

export interface MessageCatalog {
  source: string;
  messages: PrefabMessage[];
}

/** Payload of Tauri event `session-event`. */
export interface SessionEvent {
  sessionId: string;
  type: "state" | "error" | "log" | "rule_hit" | "send_done";
  open?: boolean;
  hsms?: string;
  message?: string;
  entry?: LogEntry;
}

export function defaultSessionConfig(overrides?: Partial<SessionConfig>): SessionConfig {
  return {
    name: "Session",
    role: "equipment",
    mode: "passive",
    ip: "127.0.0.1",
    port: 5000,
    sessionId: 10,
    t3: 45,
    t5: 10,
    t6: 5,
    t7: 10,
    t8: 5,
    linktestEnabled: false,
    linktestSeconds: 30,
    rebindIfPassive: true,
    mdln: "SECS-SIM",
    softrev: "0.1.0",
    clockType: "a16",
    ...overrides,
  };
}

export function emptyCatalog(): MessageCatalog {
  return { source: "", messages: [] };
}

export function sxFy(m: Pick<PrefabMessage, "stream" | "function">): string {
  return `S${m.stream}F${m.function}`;
}
