import { marked } from "marked";
import { REPO } from "../content/data";

export interface Asset {
  name: string;
  size: number;
  browser_download_url: string;
}
export interface Release {
  tag_name: string;
  name: string;
  body: string;
  published_at: string;
  prerelease: boolean;
  html_url: string;
  assets: Asset[];
}
export interface User {
  login: string;
  avatar_url: string;
}
export interface Label {
  name: string;
  color: string;
}
export interface Issue {
  number: number;
  title: string;
  state: string;
  body: string;
  html_url: string;
  comments: number;
  created_at: string;
  updated_at: string;
  user: User;
  labels: Label[];
  pull_request?: unknown;
}
export interface Comment {
  user: User;
  created_at: string;
  body: string;
}

// Unauthenticated visitors get 60 requests an hour, so every list is fetched once per visit.
const cache = new Map<string, unknown>();
export async function gh<T>(path: string): Promise<T> {
  if (cache.has(path)) return cache.get(path) as T;
  const r = await fetch(`https://api.github.com/repos/${REPO}/${path}`, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (!r.ok)
    throw new Error(
      r.status === 403
        ? "GitHub's hourly request limit is reached. Try again later."
        : `GitHub answered ${r.status}.`,
    );
  const data = (await r.json()) as T;
  cache.set(path, data);
  return data;
}

// Content packs share the repository; only engine releases use version tags.
const isEngineRelease = (release: Release) =>
  /^v?\d+\.\d+\.\d+(?:-|$)/.test(release.tag_name);

export async function releases() {
  const all: Release[] = [];
  for (let page = 1; ; page++) {
    const batch = await gh<Release[]>(`releases?per_page=100&page=${page}`);
    all.push(...batch.filter(isEngineRelease));
    if (batch.length < 100) return all;
  }
}

export async function latestRelease(): Promise<Release | null> {
  const ordered = (await releases()).sort(
    (a, b) => Date.parse(b.published_at) - Date.parse(a.published_at),
  );
  // Match the download page: prefer Stable when available, otherwise use the latest build.
  return ordered.find((release) => !release.prerelease) ?? ordered[0] ?? null;
}

export const version = (release: Release) => release.tag_name.replace(/^v/, "");
export const date = (iso: string) =>
  new Date(iso).toLocaleDateString(undefined, { dateStyle: "long" });
export const megabytes = (bytes: number) =>
  `${(bytes / 1048576).toFixed(0)} MB`;

function markdown(text: string) {
  return marked.parse(text, { async: false });
}

// Release notes and issues are written by anyone: strip everything active out of them.
export function untrusted(text: string) {
  const t = document.createElement("template");
  t.innerHTML = markdown(text || "");
  t.content
    .querySelectorAll("script, style, iframe, object, embed, form, link, meta")
    .forEach((e) => e.remove());
  t.content.querySelectorAll("*").forEach((e) =>
    [...e.attributes].forEach((a) => {
      if (
        /^on/i.test(a.name) ||
        (/^(href|src)$/i.test(a.name) &&
          /^\s*(javascript|data):/i.test(a.value) &&
          !/^data:image\//i.test(a.value))
      )
        e.removeAttribute(a.name);
    }),
  );
  return t.innerHTML;
}

export function ago(iso: string) {
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  for (const [n, unit] of [
    [31536000, "year"],
    [2592000, "month"],
    [86400, "day"],
    [3600, "hour"],
    [60, "minute"],
  ] as const) {
    if (s >= n) {
      const k = Math.floor(s / n);
      return `${k} ${unit}${k > 1 ? "s" : ""} ago`;
    }
  }
  return "just now";
}
