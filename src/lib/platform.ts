/** Which platform the WebView is running on.
 *
 *  Read once at import: a device does not grow a mouse, a touchscreen or a file
 *  manager mid-session.
 *
 *  These used to be hand-rolled `userAgent.includes("Android")` checks spread
 *  over four files, where "Android" stood in for "mobile" and for "touch". That
 *  reading breaks on iPadOS, which reports itself as "Macintosh" and is only
 *  distinguishable from a desktop Mac by its touch points.
 */

const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
const touchPoints = typeof navigator === "undefined" ? 0 : navigator.maxTouchPoints;

export const IS_ANDROID = ua.includes("Android");

export const IS_IOS =
  /iPad|iPhone|iPod/.test(ua) || (ua.includes("Macintosh") && touchPoints > 1);

export const IS_WINDOWS = ua.includes("Windows");

export const IS_MOBILE = IS_ANDROID || IS_IOS;

/** A coarse pointer alone is not enough — a Windows tablet in touch mode is
 *  still a desktop, and keeps its titlebar and its file manager. */
export const IS_TOUCH =
  IS_MOBILE ||
  (typeof window !== "undefined" && window.matchMedia("(pointer: coarse)").matches);

/** Reveal-in-file-manager needs a real filesystem path the OS will show. Neither
 *  mobile platform has one: a SAF project has no path at all, and iOS only
 *  exposes folders the user picked. */
export const HAS_FILE_MANAGER = !IS_MOBILE;
