import type { InstallSource } from "../api";

const CHANNELS = {
  play: { url: "https://play.google.com/store/apps/details?id=org.cullant.app" },
  website: { url: "https://cullant.lalogo.dev/download" },
};

type Channel = keyof typeof CHANNELS;

// A sideloaded APK has no installer we can tell apart, so it falls back to the website.
export function channelFor(source: InstallSource): Channel {
  if (source.installer === "com.android.vending") return "play";
  return "website";
}

export function updateUrl(source: InstallSource): string {
  return CHANNELS[channelFor(source)].url;
}
