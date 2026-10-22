const RELEASES_URL = "https://api.github.com/repos/la-lo-go/cullant/releases?per_page=10";

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

/** Version of the newest published release that has all its files, or null. */
export async function newestCompleteRelease(): Promise<string | null> {
  try {
    const response = await fetch(RELEASES_URL, { headers: { Accept: "application/vnd.github+json" } });
    if (!response.ok) return null;
    const releases: GitHubRelease[] = await response.json();
    for (const release of releases) {
      if (release.draft || release.prerelease) continue;
      const version = versionOf(release);
      if (version && isComplete(release, version)) return version;
    }
    return null;
  } catch {
    return null;
  }
}

/** True when version `a` is newer than `b`. Both are `major.minor.patch`. A malformed version never counts as newer. */
export function isNewer(a: string, b: string): boolean {
  const left = a.split(".").map(Number);
  const right = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if (left[i] !== right[i]) return left[i] > right[i];
  }
  return false;
}
