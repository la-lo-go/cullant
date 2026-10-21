/**
 * Shows a path cut in the middle, so both where a folder starts and where it
 * ends stay readable: `C:\Users\me\…\2024\Trip`. Whole segments are dropped
 * from the middle until the text fits the node's width.
 */
export function middleEllipsis(node: HTMLElement, path: string) {
  const ctx = document.createElement("canvas").getContext("2d");

  function fit() {
    const width = node.clientWidth;
    if (!ctx || width === 0) return void (node.textContent = path);
    ctx.font = getComputedStyle(node).font;
    const sep = path.includes("\\") ? "\\" : "/";
    const parts = path.split(sep);
    let text = path;
    let head = Math.ceil(parts.length / 2);
    let tail = parts.length - head;
    // Drop the segment nearest the middle, one at a time, keeping the first and
    // the last segment whatever happens.
    while (ctx.measureText(text).width > width && head + tail > 2) {
      if (head > tail && head > 1) head--;
      else if (tail > 1) tail--;
      else head--;
      text = [...parts.slice(0, head), "…", ...parts.slice(parts.length - tail)].join(sep);
    }
    node.textContent = text;
  }

  const observer = new ResizeObserver(fit);
  observer.observe(node);
  return {
    update(next: string) {
      path = next;
      fit();
    },
    destroy: () => observer.disconnect(),
  };
}
