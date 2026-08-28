/**
 * Context-menu content, as data.
 *
 * A menu is a tree of nodes and nothing else: the renderer knows how to draw and
 * navigate it, and every caller only describes what it wants. Submenus nest to
 * any depth, so a block like "Filter by…" is one node rather than a special case
 * in the component.
 *
 * Nodes are built fresh each time a menu opens, against the items it was opened
 * on — a `run` closure therefore captures its targets and needs no arguments.
 */

import type Zap from "@lucide/svelte/icons/zap";

interface NodeBase {
  label: string;
  /** Lucide icon component, drawn at the head of the row. */
  icon?: typeof Zap;
  disabled?: boolean;
}

export interface MenuItemNode extends NodeBase {
  kind: "item";
  /** Keyboard shortcut, right-aligned. Display only — the menu never binds it. */
  hint?: string;
  /** Drawn as already-applied (a filter that is on, a toggle that is set). */
  checked?: boolean;
  run: () => void;
}

export interface MenuSubmenuNode extends NodeBase {
  kind: "submenu";
  children: MenuNode[];
}

export type MenuNode =
  | MenuItemNode
  | MenuSubmenuNode
  | { kind: "sep" }
  | { kind: "header"; label: string };

export function isSelectable(node: MenuNode): node is MenuItemNode | MenuSubmenuNode {
  return (node.kind === "item" || node.kind === "submenu") && !node.disabled;
}

/**
 * Drop empty submenus, collapse runs of separators, and trim separators at both
 * ends. Builders can then emit a block unconditionally and let a section vanish
 * when the project has nothing to put in it, instead of every caller guarding
 * its own separator.
 */
export function pruneMenu(nodes: MenuNode[]): MenuNode[] {
  const out: MenuNode[] = [];
  for (const node of nodes) {
    if (node.kind === "submenu") {
      const children = pruneMenu(node.children);
      if (children.length === 0) continue;
      out.push({ ...node, children });
      continue;
    }
    if (node.kind === "sep" && (out.length === 0 || out[out.length - 1].kind === "sep")) continue;
    out.push(node);
  }
  while (out.length > 0 && out[out.length - 1].kind === "sep") out.pop();
  return out;
}
