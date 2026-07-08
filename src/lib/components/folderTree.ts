import type { ItemLite } from "../api";
import { dirOf } from "../stores/session.svelte";

export interface TreeNode {
  name: string;
  /** Full relative directory path (forward-slash separated), "" for the root. */
  path: string;
  count: number;
  children: Map<string, TreeNode>;
}

/** Build a directory tree (with per-folder file counts) from a project's items. */
export function buildFolderTree(items: ItemLite[]): TreeNode {
  const root: TreeNode = { name: "", path: "", count: 0, children: new Map() };
  for (const item of items) {
    root.count++;
    const dir = dirOf(item.relPath);
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
      child.count++;
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
