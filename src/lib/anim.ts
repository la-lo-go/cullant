/** Quick rubber-band nudge used to signal "no more items" at a list edge.
 *  `dir` is -1 (start) / +1 (end); `axis` picks horizontal or vertical. */
export function edgeBounce(node: HTMLElement, dir: number, axis: "x" | "y") {
  const shift = dir * (axis === "y" ? 12 : 16);
  const to = axis === "y" ? `translateY(${shift}px)` : `translateX(${shift}px)`;
  node.animate([{ transform: "none" }, { transform: to }, { transform: "none" }], {
    duration: 230,
    easing: "cubic-bezier(0.36, 0, 0.16, 1)",
  });
}
