import type { FlowDef, FlowCanvasNode, FlowCanvasEdge } from "../../types/flow";
import type { PrefabMessage } from "../../types/session";

function id(): string {
  return crypto.randomUUID();
}

function isH2e(dir: string): boolean {
  return dir.includes("H->E") || dir.includes("H-&gt;E");
}

function findOut(
  catalog: PrefabMessage[],
  stream: number,
  func: number,
  role: "host" | "equipment",
): PrefabMessage | undefined {
  const wantH2e = role === "host";
  return (
    catalog.find(
      (m) =>
        m.stream === stream &&
        m.function === func &&
        isH2e(m.direction) === wantH2e,
    ) ?? catalog.find((m) => m.stream === stream && m.function === func)
  );
}

function sendNode(
  nid: string,
  x: number,
  y: number,
  m: PrefabMessage | undefined,
  stream: number,
  func: number,
  direction: string,
): FlowCanvasNode {
  return {
    id: nid,
    type: "send",
    position: { x, y },
    data: {
      messageId: m?.id,
      stream: m?.stream ?? stream,
      function: m?.function ?? func,
      direction: m?.direction ?? direction,
      messageName: m?.messageName ?? `S${stream}F${func}`,
    },
  };
}

function edge(source: string, target: string, handle?: string): FlowCanvasEdge {
  return {
    id: id(),
    source,
    target,
    sourceHandle: handle,
  };
}

export type PresetKind = "online" | "alarm" | "recipe";

export function buildPreset(kind: PresetKind, catalog: PrefabMessage[]): FlowDef {
  const t1 = id();
  const x = 200;
  if (kind === "online") {
    const s13 = id();
    const br = id();
    const s17 = id();
    return {
      id: id(),
      name: "",
      enabled: false,
      nodes: [
        {
          id: t1,
          type: "trigger",
          position: { x, y: 20 },
          data: { kind: "onSelected" },
        },
        sendNode(s13, x, 120, findOut(catalog, 1, 13, "host"), 1, 13, "H->E"),
        {
          id: br,
          type: "branch",
          position: { x, y: 220 },
          data: {
            combinator: "and",
            clauses: [{ field: "ack", op: "eq", value: "0" }],
          },
        },
        sendNode(s17, x, 340, findOut(catalog, 1, 17, "host"), 1, 17, "H->E"),
      ],
      edges: [edge(t1, s13), edge(s13, br), edge(br, s17, "then")],
    };
  }
  if (kind === "alarm") {
    const s5 = id();
    return {
      id: id(),
      name: "",
      enabled: false,
      nodes: [
        {
          id: t1,
          type: "trigger",
          position: { x, y: 20 },
          data: { kind: "manual" },
        },
        sendNode(s5, x, 120, findOut(catalog, 5, 1, "equipment"), 5, 1, "H<-E"),
      ],
      edges: [edge(t1, s5)],
    };
  }
  const s73 = id();
  const br = id();
  const s717 = id();
  return {
    id: id(),
    name: "",
    enabled: false,
    nodes: [
      {
        id: t1,
        type: "trigger",
        position: { x, y: 20 },
        data: { kind: "manual" },
      },
      sendNode(s73, x, 120, findOut(catalog, 7, 3, "host"), 7, 3, "H->E"),
      {
        id: br,
        type: "branch",
        position: { x, y: 220 },
        data: {
          combinator: "and",
          clauses: [{ field: "ack", op: "eq", value: "0" }],
        },
      },
      sendNode(s717, x, 340, findOut(catalog, 7, 17, "host"), 7, 17, "H->E"),
    ],
    edges: [edge(t1, s73), edge(s73, br), edge(br, s717, "then")],
  };
}

export function blankFlow(name: string): FlowDef {
  return {
    id: id(),
    name,
    enabled: false,
    nodes: [
      {
        id: id(),
        type: "trigger",
        position: { x: 200, y: 40 },
        data: { kind: "manual" },
      },
    ],
    edges: [],
  };
}
