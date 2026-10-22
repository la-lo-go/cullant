const RELEASES_URL = "https://api.github.com/repos/la-lo-go/cullant/releases?per_page=10";
const NOTES_ORIGIN = "https://github.com";
const NOTES_PATH = "/la-lo-go/cullant/releases/";

// This list must match `.github/workflows/release.yml` and `cullant-site/src/lib/Downloads.ts`.
// The workflow publishes a release first and attaches the files about 30 minutes later.
// The prompt must never send the user to a page that still shows the old version.
const ASSET_TEMPLATES = [
  "Cullant_{v}_x64-setup.exe",
  "Cullant_{v}_x64_en-US.msi",
  "Cullant_{v}_amd64.deb",
  "Cullant_{v}_amd64.AppImage",
  "Cullant_{v}_aarch64.dmg",
  "cullant-{v}-arm64.apk",
];

interface GitHubRelease {
  tag_name?: string;
  draft?: boolean;
  prerelease?: boolean;
  html_url?: string;
  assets?: Array<{ name?: string }>;
}

function versionOf(release: GitHubRelease): string | null {
  const version = release.tag_name?.replace(/^v/, "");
  return version && /^\d+\.\d+\.\d+$/.test(version) ? version : null;
}

function isComplete(release: GitHubRelease, version: string): boolean {
  const names = new Set((release.assets ?? []).map((asset) => asset.name));
  return ASSET_TEMPLATES.every((template) => names.has(template.replace("{v}", version)));
}

export interface NewestRelease {
  version: string;
  /** Release page with the notes, or null when the API gave no trusted URL. */
  notesUrl: string | null;
}

// The URL comes from the network and is opened outside the app. Parse it first:
// the parser resolves "../" and "%2e%2e", so the check sees the path that is really opened.
function notesUrlOf(release: GitHubRelease): string | null {
  try {
    const url = new URL(release.html_url ?? "");
    return url.origin === NOTES_ORIGIN && url.pathname.startsWith(NOTES_PATH) ? url.href : null;
  } catch {
    return null;
  }
}

/** The newest published release that has all its files, or null. */
export async function newestCompleteRelease(): Promise<NewestRelease | null> {
  try {
    const response = await fetch(RELEASES_URL, { headers: { Accept: "application/vnd.github+json" } });
    if (!response.ok) return null;
    const releases: GitHubRelease[] = await response.json();
    for (const release of releases) {
      if (release.draft || release.prerelease) continue;
      const version = versionOf(release);
      if (version && isComplete(release, version)) return { version, notesUrl: notesUrlOf(release) };
    }
    return null;
  } catch {
    return null;
  }
}

/** True when version `a` is newer than `b`. Both are `major.minor.patch`. The first part that differs decides. A part that is not a number never counts as greater. */
export function isNewer(a: string, b: string): boolean {
  const left = a.split(".").map(Number);
  const right = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if (left[i] !== right[i]) return left[i] > right[i];
  }
  return false;
}
