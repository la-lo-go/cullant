import { IS_ANDROID, IS_WINDOWS } from "../platform";

export const HEVC_FREE_URL = "https://apps.microsoft.com/detail/9n4wgh0z6vhq";
export const HEVC_PAID_URL = "https://apps.microsoft.com/detail/9nmzlz57r3t7";
const VLC_URL = "https://www.videolan.org/vlc/";

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

export interface PlaybackLink {
  name: string;
  url: string;
}

export interface PlaybackReason {
  text: string;
  steps: string[];
  links: PlaybackLink[];
  /** Set only when an installation fixes in-app playback. */
  install?: PlaybackLink;
}

const VLC_LINK = { name: "Get VLC", url: VLC_URL };

function externalPlayer(text: string): PlaybackReason {
  return {
    text,
    steps: [
      "Select \"Open in external player\" to play the video in the default player of this device.",
      `If that player cannot play it either, install VLC${IS_ANDROID ? " from the Play Store" : ""}. VLC plays almost all video formats.`,
    ],
    links: [VLC_LINK],
  };
}

/** The reason for a codec that the media pipeline refused outright. */
export function codecReason(codec: string): PlaybackReason {
  const name = NAMES[codec.split(".")[0]] ?? codec;
  if (name !== "HEVC") return externalPlayer(`This device has no ${name} decoder for in-app playback.`);
  if (hevcSupported()) {
    return externalPlayer("This video uses an HEVC profile that this device cannot decode.");
  }
  if (!IS_WINDOWS) return externalPlayer("This device has no HEVC decoder for in-app playback.");
  return {
    text: "This video uses HEVC. Windows needs the HEVC Video Extensions to play it.",
    steps: [
      "Install the HEVC Video Extensions from the Microsoft Store. The free version is enough.",
      "If the Store does not let you install the free version, install the paid version.",
      "Restart Cullant.",
    ],
    links: [
      { name: "Free version", url: HEVC_FREE_URL },
      { name: "Paid version", url: HEVC_PAID_URL },
    ],
    install: { name: "Install HEVC", url: HEVC_FREE_URL },
  };
}
