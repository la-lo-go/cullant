/** Quick rubber-band nudge used to signal "no more items" at a list edge.
 *  `dir` is -1 (start) / +1 (end); `axis` picks horizontal or vertical. */

// Track the in-flight bounce per node so a burst of blocked navigations cancels
// the previous animation instead of stacking transforms on top of each other.
const running = new WeakMap<HTMLElement, Animation>();

export function edgeBounce(node: HTMLElement, dir: number, axis: "x" | "y") {
  // Resist the direction of travel: pressing "next" (dir=+1) at the last item
  // pulls the content toward the start (negative), the opposite of how a
  // successful forward transition would slide it — so it reads as a wall.
  const shift = -dir * (axis === "y" ? 12 : 16);
  const to = axis === "y" ? `translateY(${shift}px)` : `translateX(${shift}px)`;

  running.get(node)?.cancel();
  const anim = node.animate(
    [{ transform: "none" }, { transform: to }, { transform: "none" }],
    {
      duration: 230,
      easing: "cubic-bezier(0.36, 0, 0.16, 1)",
    },
  );
  running.set(node, anim);
}
