import { DOCS, SITE } from "../content/data";

export const BASE = import.meta.env.BASE_URL;

export const url = (path: string) => BASE + path.replace(/^\//, "");

export const absolute = (path: string) => SITE + path.replace(/^\//, "");

export const docPath = (file: string, anchor?: string) => {
  const doc = DOCS.find((d) => d.file === file);
  return `/docs/${doc?.slug ?? file.toLowerCase()}/${anchor ? `#${anchor}` : ""}`;
};

export interface Route {
  path: string;
  anchor?: string;
}

export function parse(pathname: string, hash = ""): Route {
  const rest = pathname.startsWith(BASE)
    ? pathname.slice(BASE.length)
    : pathname.replace(/^\//, "");
  return {
    path: `/${rest}`.replace(/\/+$/, "") || "/",
    anchor: decodeURIComponent(hash.replace(/^#/, "")) || undefined,
  };
}

// Links from before the site had real paths look like #/docs/USER_GUIDE#section.
export function legacy(hash: string) {
  if (!hash.startsWith("#/")) return null;
  const [path, anchor] = hash.slice(2).split("#");
  const doc = path.match(/^docs\/(\w+)/);
  if (doc) return url(docPath(doc[1], anchor));
  return url(path ? `${path}${path.startsWith("issues/") ? "" : "/"}` : "");
}

export function navigate(href: string) {
  history.pushState(null, "", href);
  window.dispatchEvent(new Event("navigate"));
}
