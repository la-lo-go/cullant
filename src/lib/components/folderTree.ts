import type { ItemLite } from "../api";
import { dirOf } from "../stores/session.svelte";

export interface TreeNode {
  name: string;
  /** Full relative directory path (forward-slash separated), "" for the root. */
  path: string;
  count: number;
  children: Map<string, TreeNode>;
}

export function buildFolderTree(items: ItemLite[], ignored: (path: string) => boolean = () => false): TreeNode {
  const root: TreeNode = { name: "", path: "", count: 0, children: new Map() };
  for (const item of items) {
    const dir = dirOf(item.relPath);
    const visible = !ignored(dir);
    if (visible) root.count++;
    if (!dir) continue;
    let node = root;
    let acc = "";
    for (const segment of dir.split("/")) {
      acc = acc ? `${acc}/${segment}` : segment;
      let child = node.children.get(segment);
      if (!child) {
        child = { name: segment, path: acc, count: 0, children: new Map() };
        node.children.set(segment, child);
      }
      if (visible) child.count++;
      node = child;
    }
  }
  return root;
}

export function collectFolderPaths(node: TreeNode, out: Set<string> = new Set()): Set<string> {
  out.add(node.path);
  for (const child of node.children.values()) collectFolderPaths(child, out);
  return out;
}

export function hasCollapsedBranch(node: TreeNode, collapsed: ReadonlySet<string>): boolean {
  return node.children.size > 0 && (collapsed.has(node.path) ||
    [...node.children.values()].some((child) => hasCollapsedBranch(child, collapsed)));
}

export function visibleFolderPaths(node: TreeNode, collapsed: ReadonlySet<string>, out: string[] = []): string[] {
  for (const child of node.children.values()) {
    out.push(child.path);
    if (!collapsed.has(child.path)) visibleFolderPaths(child, collapsed, out);
  }
  return out;
}
