import { api, type RuntimeToolStatus } from "../api";
import { HEVC_FREE_URL, HEVC_PAID_URL, hevcSupported } from "../video/codecs";

export interface RuntimeToolLink {
  name: string;
  url: string;
}

/** One installation, with every feature that it restores. */
export interface RuntimeToolWarning {
  id: string;
  name: string;
  detail: string;
  effects: string[];
  links: RuntimeToolLink[];
}

/** One missing capability. The id is what "Don't show again" stores. */
interface Issue {
  id: string;
  install: "ffmpeg" | "windows";
  effect: string;
  links?: RuntimeToolLink[];
}

const HIDDEN_KEY = "cullant.hiddenRuntimeTools";
const FFMPEG_LINK = { name: "FFmpeg downloads", url: "https://ffmpeg.org/download.html" };
const HEIF_LINK = { name: "HEIF Image Extensions", url: "https://apps.microsoft.com/detail/9pmmsr1cgpwg" };
const HEVC_LINKS = [
  { name: "HEVC Video Extensions (free)", url: HEVC_FREE_URL },
  { name: "Paid version, if the free one does not install", url: HEVC_PAID_URL },
];

const FFMPEG_STEPS: Record<string, string> = {
  windows: "Run \"winget install Gyan.FFmpeg\" in a terminal. Or download a build and add its bin folder to PATH.",
  macos: "Run \"brew install ffmpeg\" in a terminal.",
  linux: "Install the ffmpeg package of your distribution.",
};

function hiddenTools(): Set<string> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? "[]");
    return new Set(Array.isArray(saved) ? saved.filter(value => typeof value === "string") : []);
  } catch {
    return new Set();
  }
}

function heifIssue(status: RuntimeToolStatus): Issue {
  if (status.platform === "windows") return {
    id: "heif", install: "windows",
    effect: "Thumbnails and previews of HEIF photos. As an alternative, install FFmpeg 7 or later.",
    links: [HEIF_LINK, ...HEVC_LINKS],
  };
  const old = status.ffmpegPresent && status.ffmpegMajor !== null ? ` This system has FFmpeg ${status.ffmpegMajor}.` : "";
  return { id: "heif", install: "ffmpeg", effect: `Thumbnails and previews of HEIF photos. They need FFmpeg 7 or later.${old}` };
}

function issues(status: RuntimeToolStatus): Issue[] {
  if (status.platform === "android" || status.platform === "ios") return [];
  const result: Issue[] = [];
  if (status.hasVideos && !status.ffmpegPresent) result.push({
    id: "ffmpeg", install: "ffmpeg", effect: "Video thumbnails in the grid and poster frames in the loupe.",
  });
  if (status.hasVideos && !status.ffprobePresent) result.push({
    id: "ffprobe", install: "ffmpeg",
    effect: "Video capture dates, dimensions, codecs, frame rates and durations, and the filters that use them. Rescan the project after installation.",
  });
  if (status.hasHeif && !status.heifAvailable) result.push(heifIssue(status));
  if (status.platform === "windows" && status.hasHevc && !hevcSupported()) result.push({
    id: "hevc", install: "windows",
    effect: "In-app playback of HEVC videos. You can still open them in an external player.",
    links: HEVC_LINKS,
  });
  return result;
}

function warning(install: Issue["install"], platform: string, group: Issue[]): RuntimeToolWarning {
  const links = [...new Map(group.flatMap(issue => issue.links ?? []).map(link => [link.url, link])).values()];
  const effects = group.map(issue => issue.effect);
  if (install === "windows") return {
    id: install, name: "Windows media extensions", effects, links,
    detail: "Install these extensions from the Microsoft Store, then restart Cullant.",
  };
  return {
    id: install, name: "FFmpeg", effects, links: [FFMPEG_LINK],
    detail: `${FFMPEG_STEPS[platform] ?? FFMPEG_STEPS.linux} FFprobe comes with FFmpeg. Restart Cullant after installation.`,
  };
}

class RuntimeToolsStore {
  pending = $state<RuntimeToolWarning[]>([]);
  private pendingIds: string[] = [];
  private acknowledged = new Set<string>();
  private request = 0;

  offer(status: RuntimeToolStatus) {
    const hidden = hiddenTools();
    const open = issues(status).filter(issue => !hidden.has(issue.id) && !this.acknowledged.has(issue.id));
    this.pendingIds = open.map(issue => issue.id);
    const installs = [...new Set(open.map(issue => issue.install))];
    this.pending = installs.map(install => warning(install, status.platform, open.filter(issue => issue.install === install)));
  }

  dismiss(hide: boolean) {
    const hidden = hiddenTools();
    for (const id of this.pendingIds) {
      this.acknowledged.add(id);
      if (hide) hidden.add(id);
    }
    if (hide) {
      try { localStorage.setItem(HIDDEN_KEY, JSON.stringify([...hidden])); } catch { /* Storage is optional. */ }
    }
    this.pendingIds = [];
    this.pending = [];
  }

  async check() {
    const request = ++this.request;
    try {
      const status = await api.runtimeTools();
      if (request === this.request) this.offer(status);
    } catch {
      // A closing project can remove the database while the probe is running.
    }
  }

  cancel() {
    this.request++;
    this.pendingIds = [];
    this.pending = [];
  }
}

export const runtimeTools = new RuntimeToolsStore();
