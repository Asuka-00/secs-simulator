/**
 * SECS-II body tree helpers: SML parse/format, labels, mutations.
 * SECS-II 正文树工具：SML 解析/格式化、标签、增删改。
 */
import type { BodyItem } from "../types/session";

const ITEM_TYPES = [
  "L",
  "A",
  "B",
  "BOOLEAN",
  "U1",
  "U2",
  "U4",
  "U8",
  "I1",
  "I2",
  "I4",
  "I8",
  "F4",
  "F8",
] as const;

export type ItemType = (typeof ITEM_TYPES)[number];

export { ITEM_TYPES };

export function newId(): string {
  return crypto.randomUUID?.() ?? `n-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

export function newLeaf(type: string = "U2", name = "", value = "0"): BodyItem {
  return { id: newId(), type, name, value, children: [] };
}

export function newList(name = "", children: BodyItem[] = []): BodyItem {
  return { id: newId(), type: "L", name, value: "", children };
}

export function cloneTree(items: BodyItem[]): BodyItem[] {
  return items.map((n) => ({
    ...n,
    id: newId(),
    children: cloneTree(n.children ?? []),
  }));
}

/** GWGEM-style label: `L L 3 RPTIDCOUNT` / `U U2 1 DATAID '0'` */
export function bodyItemLabel(n: BodyItem): string {
  const ty = (n.type || "L").toUpperCase();
  const letter = ty === "BOOLEAN" ? "B" : ty.charAt(0);
  if (ty === "L") {
    const count = n.children?.length ?? 0;
    const nm = n.name?.trim() ? ` ${n.name}` : "";
    return `L L ${count}${nm}`;
  }
  const vals = (n.value ?? "").trim().split(/\s+/).filter(Boolean);
  const count = vals.length || (ty === "A" ? (n.value?.length ?? 0) : 0);
  const nm = n.name?.trim() ? ` ${n.name}` : "";
  let valPart = "";
  if (ty === "A") {
    valPart = ` "${n.value ?? ""}"`;
  } else if (vals.length) {
    valPart = ` '${vals.join(" ")}'`;
  }
  return `${letter} ${ty} ${count}${nm}${valPart}`;
}

/** Parse compact SML body fragment into a single-root or multi-root tree. */
export function parseSmlBodyToTree(sml: string): BodyItem[] {
  const t = sml.trim();
  if (!t) return [];
  try {
    const p = new SmlBodyParser(t);
    const items: BodyItem[] = [];
    p.skipWs();
    while (!p.eof()) {
      if (p.peek() !== "<") break;
      items.push(p.parseItem());
      p.skipWs();
    }
    return items;
  } catch {
    // Fallback: one opaque A leaf with raw text
    return [newLeaf("A", "", t)];
  }
}

export function bodyTreeToSml(items: BodyItem[]): string {
  if (!items.length) return "";
  if (items.length === 1) return formatItem(items[0]);
  return `<L ${items.map(formatItem).join(" ")}>`;
}

function formatItem(n: BodyItem): string {
  const ty = (n.type || "L").toUpperCase();
  if (ty === "L") {
    const kids = (n.children ?? []).map(formatItem).join(" ");
    return kids ? `<L ${kids}>` : "<L>";
  }
  if (ty === "A") {
    const v = (n.value ?? "").replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    return `<A "${v}">`;
  }
  if (ty === "BOOLEAN") {
    const v = (n.value ?? "").trim() || "false";
    return `<BOOLEAN ${v}>`;
  }
  if (ty === "B") {
    const v = (n.value ?? "").trim();
    return v ? `<B ${v}>` : "<B>";
  }
  const v = (n.value ?? "").trim();
  return v ? `<${ty} ${v}>` : `<${ty} 0>`;
}

/** Ensure message has a bodyTree (from field or parsed SML). */
export function ensureBodyTree(bodyTree: BodyItem[] | undefined, bodySml: string): BodyItem[] {
  if (bodyTree && bodyTree.length > 0) {
    return JSON.parse(JSON.stringify(bodyTree)) as BodyItem[];
  }
  return parseSmlBodyToTree(bodySml ?? "");
}

// ── minimal SML item parser ─────────────────────────────────

class SmlBodyParser {
  private i = 0;
  constructor(private s: string) {}

  eof() {
    return this.i >= this.s.length;
  }
  peek() {
    return this.s[this.i];
  }
  bump() {
    this.i++;
  }
  skipWs() {
    while (this.i < this.s.length && /\s/.test(this.s[this.i])) this.i++;
  }

  parseItem(): BodyItem {
    this.skipWs();
    if (this.peek() !== "<") throw new Error("expected <");
    this.bump();
    this.skipWs();
    const type = this.readType();
    this.skipSizeBracket();
    this.skipWs();
    if (type === "L") {
      const children: BodyItem[] = [];
      while (this.peek() === "<") {
        children.push(this.parseItem());
        this.skipWs();
      }
      if (this.peek() !== ">") throw new Error("unterminated L");
      this.bump();
      return newList("", children);
    }
    if (type === "A") {
      const value = this.readAsciiValue();
      this.skipWs();
      if (this.peek() !== ">") throw new Error("unterminated A");
      this.bump();
      return newLeaf("A", "", value);
    }
    // numerics / binary / boolean
    const parts: string[] = [];
    while (!this.eof() && this.peek() !== ">") {
      this.skipWs();
      if (this.peek() === ">") break;
      parts.push(this.readToken());
      this.skipWs();
    }
    if (this.peek() !== ">") throw new Error(`unterminated ${type}`);
    this.bump();
    return newLeaf(type, "", parts.join(" "));
  }

  readType(): string {
    const start = this.i;
    while (this.i < this.s.length && /[A-Za-z0-9]/.test(this.s[this.i])) this.i++;
    if (start === this.i) throw new Error("type");
    return this.s.slice(start, this.i).toUpperCase();
  }

  skipSizeBracket() {
    this.skipWs();
    if (this.peek() !== "[") return;
    this.bump();
    while (this.i < this.s.length && this.peek() !== "]") this.i++;
    if (this.peek() === "]") this.bump();
  }

  readAsciiValue(): string {
    let out = "";
    while (!this.eof() && this.peek() !== ">") {
      this.skipWs();
      if (this.peek() === ">") break;
      if (this.peek() === '"') {
        this.bump();
        const start = this.i;
        while (this.i < this.s.length && this.s[this.i] !== '"') this.i++;
        out += this.s.slice(start, this.i);
        if (this.peek() === '"') this.bump();
      } else {
        // bare token / 0xNN
        out += String.fromCharCode(parseInt(this.readToken(), 16) || 0);
      }
    }
    return out;
  }

  readToken(): string {
    const start = this.i;
    while (this.i < this.s.length && !/\s|>/.test(this.s[this.i])) this.i++;
    return this.s.slice(start, this.i);
  }
}

/** Find node by id in forest. */
export function findNode(
  items: BodyItem[],
  id: string,
): { node: BodyItem; parent: BodyItem | null; index: number } | null {
  for (let i = 0; i < items.length; i++) {
    if (items[i].id === id) return { node: items[i], parent: null, index: i };
    const hit = findInChildren(items[i], id);
    if (hit) return hit;
  }
  return null;
}

function findInChildren(
  parent: BodyItem,
  id: string,
): { node: BodyItem; parent: BodyItem; index: number } | null {
  const kids = parent.children ?? [];
  for (let i = 0; i < kids.length; i++) {
    if (kids[i].id === id) return { node: kids[i], parent, index: i };
    const hit = findInChildren(kids[i], id);
    if (hit) return hit;
  }
  return null;
}

export function removeNode(items: BodyItem[], id: string): BodyItem[] {
  const next = JSON.parse(JSON.stringify(items)) as BodyItem[];
  const hit = findNode(next, id);
  if (!hit) return next;
  if (!hit.parent) {
    next.splice(hit.index, 1);
  } else {
    hit.parent.children.splice(hit.index, 1);
  }
  return next;
}

export function addChild(items: BodyItem[], parentId: string | null, child: BodyItem): BodyItem[] {
  const next = JSON.parse(JSON.stringify(items)) as BodyItem[];
  if (!parentId) {
    next.push(child);
    return next;
  }
  const hit = findNode(next, parentId);
  if (!hit) return next;
  if (hit.node.type.toUpperCase() !== "L") {
    // promote non-list? skip
    return next;
  }
  hit.node.children = hit.node.children ?? [];
  hit.node.children.push(child);
  return next;
}
