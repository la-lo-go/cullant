/**
 * The five colour labels and the colours they are drawn in.
 *
 * The swatch colours had been copied into every component that draws one, which
 * is why the radial menu ended up showing a generic palette icon for "Yellow
 * label" while the labels group two levels in showed the actual yellow: the
 * colours simply were not reachable from there. One list, one place.
 */

export const LABELS = ["Red", "Yellow", "Green", "Blue", "Purple"] as const;
export type Label = (typeof LABELS)[number];

export const LABEL_COLORS: Record<string, string> = {
  Red: "#e05555",
  Yellow: "#e0c34f",
  Green: "#59b85e",
  Blue: "#5588e0",
  Purple: "#9a66d6",
};
