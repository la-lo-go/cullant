import { api, type RuntimeToolStatus } from "../api";

export interface RuntimeToolWarning {
  id: string;
  name: string;
  detail: string;
  url: string;
  extraLink?: { name: string; url: string };
}

const HIDDEN_KEY = "cullant.hiddenRuntimeTools";
const FFMPEG_URL = "https://ffmpeg.org/download.html";
const HEIF_URL = "https://apps.microsoft.com/detail/9pmmsr1cgpwg";
const HEVC_URL = "https://apps.microsoft.com/detail/9nmzlz57r3t7";

function hiddenTools(): Set<string> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? "[]");
    return new Set(Array.isArray(saved) ? saved.filter(value => typeof value === "string") : []);
  } catch {
    return new Set();
  }
}

function hevcSupported(): boolean {
  const type = 'video/mp4; codecs="hvc1.1.6.L93.B0"';
  return !!document.createElement("video").canPlayType(type) ||
    typeof MediaSource !== "undefined" && MediaSource.isTypeSupported(type);
}

function heifWarning(platform: string): RuntimeToolWarning {
  return platform === "windows" ? {
    id: "heif", name: "HEIF and HEVC extensions",
    detail: "No HEIF decoder is available. Install both Windows extensions, or install FFmpeg 7 or later. Restart Cullant after installation.",
    url: HEIF_URL, extraLink: { name: "HEVC Video Extensions", url: HEVC_URL },
  } : {
    id: "heif", name: "FFmpeg for HEIF",
    detail: "HEIF photos need FFmpeg 7 or later on this system. Install it and add its bin folder to PATH. Restart Cullant after installation.",
    url: FFMPEG_URL,
  };
}

function warnings(status: RuntimeToolStatus): RuntimeToolWarning[] {
  if (status.platform === "android" || status.platform === "ios") return [];
  const result: RuntimeToolWarning[] = [];
  if (status.hasVideos && !status.ffmpegPresent) result.push({
    id: "ffmpeg", name: "FFmpeg",
    detail: "Video thumbnails need FFmpeg. Install it for your system and add its bin folder to PATH. Restart Cullant after installation.",
    url: FFMPEG_URL,
  });
  if (status.hasVideos && !status.ffprobePresent) result.push({
    id: "ffprobe", name: "FFprobe",
    detail: "Video dates, dimensions, codecs, frame rates and durations need FFprobe. It comes with FFmpeg. Add its bin folder to PATH, then restart Cullant and rescan this project.",
    url: FFMPEG_URL,
  });
  if (status.hasHeif && !status.heifAvailable) result.push(heifWarning(status.platform));
  if (status.platform === "windows" && status.hasHevc && !hevcSupported()) result.push({
    id: "hevc", name: "HEVC Video Extensions",
    detail: "This project has HEVC videos. This WebView does not report HEVC playback support. Install the Windows extension, then restart Cullant. Some video profiles can still need an external player.",
    url: HEVC_URL,
  });
  return result;
}

class RuntimeToolsStore {
  pending = $state<RuntimeToolWarning[]>([]);
  private acknowledged = new Set<string>();
  private request = 0;

  offer(issues: RuntimeToolWarning[]) {
    const hidden = hiddenTools();
    this.pending = issues.filter(issue => !hidden.has(issue.id) && !this.acknowledged.has(issue.id));
  }

  dismiss(hide: boolean) {
    const hidden = hiddenTools();
    for (const issue of this.pending) {
      this.acknowledged.add(issue.id);
      if (hide) hidden.add(issue.id);
    }
    if (hide) {
      try { localStorage.setItem(HIDDEN_KEY, JSON.stringify([...hidden])); } catch { /* Storage is optional. */ }
    }
    this.pending = [];
  }

  async check() {
    const request = ++this.request;
    try {
      const status = await api.runtimeTools();
      if (request === this.request) this.offer(warnings(status));
    } catch {
      // A closing project can remove the database while the probe is running.
    }
  }

  cancel() {
    this.request++;
    this.pending = [];
  }
}

export const runtimeTools = new RuntimeToolsStore();
