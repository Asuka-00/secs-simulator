/** Vue Flow graph persisted on a session. */

export type FlowNodeType = "trigger" | "send" | "delay" | "wait" | "branch";
export type TriggerKind = "manual" | "onSelected" | "onInbound" | "interval";
export type BranchField = "sxFy" | "ack" | "bodyPath" | "sml";
export type BranchOp = "eq" | "ne" | "gt" | "lt" | "contains";

export interface FlowPos {
  x: number;
  y: number;
}

export interface BranchClause {
  nodeId?: string;
  field: BranchField;
  path?: string;
  op: BranchOp;
  value: string;
}

export interface TriggerData {
  kind: TriggerKind;
  stream?: number;
  function?: number;
  intervalMs?: number;
}

export interface SendData {
  messageId?: string;
  stream?: number;
  function?: number;
  direction?: string;
  messageName?: string;
}

export interface DelayData {
  ms: number;
}

export interface WaitData {
  stream: number;
  function: number;
  timeoutMs: number;
}

export interface BranchData {
  combinator: "and" | "or";
  clauses: BranchClause[];
}

export type FlowNodeData =
  | TriggerData
  | SendData
  | DelayData
  | WaitData
  | BranchData;

export interface FlowCanvasNode {
  id: string;
  type: FlowNodeType;
  position: FlowPos;
  data: Record<string, unknown>;
  class?: string;
}

export interface FlowCanvasEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string | null;
  targetHandle?: string | null;
}

export interface FlowDef {
  id: string;
  name: string;
  enabled: boolean;
  nodes: FlowCanvasNode[];
  edges: FlowCanvasEdge[];
}

export interface FlowTick {
  sessionId: string;
  type: "flow_progress" | "flow_done";
  flowId?: string;
  nodeId?: string;
  message?: string;
}

export const FLOW_DND = "application/secs-flow";
