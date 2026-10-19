import { IS_WINDOWS } from "../platform";

export const HEVC_FREE_URL = "https://apps.microsoft.com/detail/9n4wgh0z6vhq";
export const HEVC_PAID_URL = "https://apps.microsoft.com/detail/9nmzlz57r3t7";

const NAMES: Record<string, string> = {
  avc1: "H.264", avc3: "H.264", hvc1: "HEVC", hev1: "HEVC",
  vp08: "VP8", vp8: "VP8", vp09: "VP9", av01: "AV1",
};

/** Main profile, 8-bit: what the Windows extension adds when it is installed. */
export function hevcSupported(): boolean {
  const type = 'video/mp4; codecs="hvc1.1.6.L93.B0"';
  return !!document.createElement("video").canPlayType(type) ||
    typeof MediaSource !== "undefined" && MediaSource.isTypeSupported(type);
}

export interface PlaybackReason {
  text: string;
  /** Set only when an installation fixes the failure. */
  install?: string;
}

/** The reason for a codec that the media pipeline refused outright. */
export function codecReason(codec: string): PlaybackReason {
  const name = NAMES[codec.split(".")[0]] ?? codec;
  if (name !== "HEVC") return { text: `This device cannot decode ${name} video.` };
  if (hevcSupported()) return { text: "This device cannot decode this HEVC profile." };
  if (IS_WINDOWS) {
    return {
      text: "The video is HEVC, and the HEVC Video Extensions are not installed. Install them, then restart Cullant.",
      install: HEVC_FREE_URL,
    };
  }
  return { text: "This device has no HEVC decoder." };
}
