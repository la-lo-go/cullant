export interface ZoomSnapshot {
  zoomed: boolean;
  /** Source pixels per CSS pixel. A value of 1 is a physical 1:1 view. */
  scale: number;
  /** Image-relative centre, independent of viewport and image dimensions. */
  cx: number;
  cy: number;
}

export const FIT_ZOOM: ZoomSnapshot = {
  zoomed: false,
  scale: 1,
  cx: 0.5,
  cy: 0.5,
};

export function sameZoom(a: ZoomSnapshot | null, b: ZoomSnapshot): boolean {
  return (
    a !== null &&
    a.zoomed === b.zoomed &&
    Math.abs(a.scale - b.scale) < 1e-6 &&
    Math.abs(a.cx - b.cx) < 1e-6 &&
    Math.abs(a.cy - b.cy) < 1e-6
  );
}

export function zoomSignature(zoom: ZoomSnapshot): string {
  return `${zoom.zoomed ? 1 : 0}:${zoom.scale}:${zoom.cx}:${zoom.cy}`;
}
