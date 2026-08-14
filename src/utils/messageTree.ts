/**
 * Build the message catalog tree (pair/variant groups + body preview).
 * 构建消息目录树（配对/变体分组 + 正文预览）。
 */
import type { BodyItem, PrefabMessage } from "../types/session";
import { sxFy } from "../types/session";
import { bodyItemLabel, ensureBodyTree } from "./secsBody";

export type TreeNodeKind = "group" | "message" | "body";

export interface MsgTreeNode {
  id: string;
  kind: TreeNodeKind;
  label: string;
  /** Group key e.g. S2F5 */
  groupKey?: string;
  stream?: number;
  function?: number;
  message?: PrefabMessage;
  children?: MsgTreeNode[];
}

/**
 * Group key for the transaction tree.
 *
 * SMD often reuses the same PairName for many variants (e.g. 46× S6F11 events
 * all with PairName="S6F11"). The real title is in Description (ALARM_SET, …).
 * Collapse only true primary/secondary pairs that share the same Description.
 */
function pairKey(m: PrefabMessage): string {
  const base = m.pairName?.trim()
    ? m.pairName.trim()
    : (() => {
        const f = m.function % 2 === 0 ? m.function - 1 : m.function;
        return `S${m.stream}F${Math.max(0, f)}`;
      })();
  const variant = (m.description || "").trim();
  // When Description is empty or equals SxFy/pair, do not invent extra keys.
  if (!variant || variant === base || /^S\d+F\d+$/i.test(variant)) {
    return base;
  }
  return `${base}::${variant}`;
}

function sortKey(k: string): [number, number, string] {
  const head = k.split("::")[0] ?? k;
  const m = /^S(\d+)F(\d+)$/i.exec(head);
  if (m) return [Number(m[1]), Number(m[2]), k];
  return [999, 999, k];
}

function comparePair(a: string, b: string): number {
  const [sa, fa, ka] = sortKey(a);
  const [sb, fb, kb] = sortKey(b);
  if (sa !== sb) return sa - sb;
  if (fa !== fb) return fa - fb;
  return ka.localeCompare(kb, undefined, { numeric: true, sensitivity: "base" });
}

function compareMsg(a: PrefabMessage, b: PrefabMessage): number {
  if (a.stream !== b.stream) return a.stream - b.stream;
  if (a.function !== b.function) return a.function - b.function;
  // H->E before H<-E for stable order
  const d = a.direction.localeCompare(b.direction);
  if (d !== 0) return d;
  return (a.description || "").localeCompare(b.description || "");
}

/** Display title: prefer Description when MessageName is just SxFy. */
export function messageTitle(m: PrefabMessage): string {
  const sx = sxFy(m);
  const desc = (m.description || "").trim();
  const name = (m.messageName || "").trim();
  if (desc && desc !== sx && desc !== name) return desc;
  if (name && name !== sx) return name;
  if (desc) return desc;
  if (name) return name;
  return sx;
}

function bodyPreviewNodes(m: PrefabMessage): MsgTreeNode[] {
  const tree = ensureBodyTree(m.bodyTree, m.bodySml ?? "");
  return tree.map((item, idx) => bodyItemToTreeNode(m, item, idx));
}

function bodyItemToTreeNode(m: PrefabMessage, item: BodyItem, idx: number): MsgTreeNode {
  const kids = (item.children ?? []).map((c, i) =>
    bodyItemToTreeNode(m, c, i),
  );
  return {
    id: `body:${m.id}:${item.id || idx}`,
    kind: "body",
    label: bodyItemLabel(item),
    message: m,
    children: kids.length ? kids : undefined,
  };
}

export function messageLeafLabel(m: PrefabMessage): string {
  const w = m.wait ? " W" : "";
  const r = m.autoReply ? " <R>" : "";
  const title = messageTitle(m);
  return `${sxFy(m)}: ${title}${w}${r} ${m.direction}`;
}

export function buildMessageTree(messages: PrefabMessage[]): MsgTreeNode[] {
  const map = new Map<string, PrefabMessage[]>();
  for (const m of messages) {
    const key = pairKey(m);
    const list = map.get(key);
    if (list) list.push(m);
    else map.set(key, [m]);
  }

  const keys = [...map.keys()].sort(comparePair);
  return keys.map((key) => {
    const msgs = (map.get(key) ?? []).slice().sort(compareMsg);
    const primary =
      msgs.find((m) => m.function % 2 === 1) ??
      msgs.find((m) => m.wait) ??
      msgs[0];
    const stream = primary?.stream ?? 0;
    const fn = primary?.function ?? 0;
    // e.g. "S6F11 - CARRIER_ID_ERROR" (match classic SMD library UI)
    const sx = primary ? sxFy(primary) : key.split("::")[0] ?? key;
    const title = primary ? messageTitle(primary) : key;
    const groupLabel = title && title !== sx ? `${sx} - ${title}` : sx;

    const children: MsgTreeNode[] = msgs.map((m) => {
      const bodyKids = bodyPreviewNodes(m);
      return {
        id: m.id,
        kind: "message" as const,
        label: messageLeafLabel(m),
        stream: m.stream,
        function: m.function,
        message: m,
        children: bodyKids.length ? bodyKids : undefined,
      };
    });

    return {
      id: `g:${key}`,
      kind: "group" as const,
      label: groupLabel,
      groupKey: key,
      stream,
      function: fn,
      message: primary,
      children,
    };
  });
}

export function filterTree(nodes: MsgTreeNode[], q: string): MsgTreeNode[] {
  const query = q.trim().toLowerCase();
  if (!query) return nodes;

  const filterNode = (n: MsgTreeNode): MsgTreeNode | null => {
    if (n.kind === "body") {
      return n.label.toLowerCase().includes(query) ? n : null;
    }
    const selfHit =
      n.label.toLowerCase().includes(query) ||
      (n.message &&
        `${n.message.messageName} ${n.message.description} ${sxFy(n.message)} ${n.message.direction}`
          .toLowerCase()
          .includes(query));
    const kids = (n.children ?? [])
      .map(filterNode)
      .filter((x): x is MsgTreeNode => x !== null);
    if (selfHit || kids.length) {
      return { ...n, children: kids.length ? kids : n.children };
    }
    return null;
  };

  return nodes.map(filterNode).filter((x): x is MsgTreeNode => x !== null);
}

export function isHostToEquip(direction: string): boolean {
  return direction.includes("H->E") || direction.includes("H→E");
}
