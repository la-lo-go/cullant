/**
 * Shows a path cut in the middle, so both where a folder starts and where it
 * ends stay readable: `C:\Users\me\…\2024\Trip`. Of all the cuts that fit the
 * node's width, the one that keeps the most text wins, so a long segment in
 * the middle does not cost the short ones around it.
 */
export function middleEllipsis(node: HTMLElement, path: string) {
  const ctx = document.createElement("canvas").getContext("2d");

  function fit() {
    const width = node.clientWidth;
    node.textContent = path;
    if (!ctx || width === 0) return;
    ctx.font = getComputedStyle(node).font;
    if (ctx.measureText(path).width <= width) return;
    const sep = path.includes("\\") ? "\\" : "/";
    const parts = path.split(sep);
    // Fallback when even the first and last segment overflow: CSS clips it.
    let best = [parts[0], "…", parts[parts.length - 1]].join(sep);
    let bestScore = 0;
    for (let head = 1; head < parts.length - 1; head++) {
      for (let tail = 1; head + tail < parts.length; tail++) {
        const text = [...parts.slice(0, head), "…", ...parts.slice(-tail)].join(sep);
        // More text wins; on a tie, the cut that keeps more of the end.
        const score = text.length * 2 + (tail >= head ? 1 : 0);
        if (score > bestScore && ctx.measureText(text).width <= width) {
          best = text;
          bestScore = score;
        }
      }
    }
    node.textContent = best;
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
