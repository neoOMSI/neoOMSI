import { Marked, type Token, type Tokens } from "marked";
import { DOCS, REPO } from "./data";
import { iconMarkup } from "../components/icons";
import { docPath, url } from "../lib/routes";

const sources = import.meta.glob<string>("../../../docs/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
});

export const source = (file: string): string | undefined =>
  sources[`../../../docs/${file}.md`];

const CALLOUTS: Record<string, [string, string]> = {
  NOTE: ["Note", "info"],
  TIP: ["Tip", "bolt"],
  IMPORTANT: ["Important", "error"],
  WARNING: ["Warning", "warning"],
  CAUTION: ["Caution", "warning"],
};

const ENTITIES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};

export const esc = (text: string) =>
  text.replace(/[&<>"']/g, (c) => ENTITIES[c]);

export const plain = (html: string) =>
  html
    .replace(/<[^>]+>/g, "")
    .replace(/&(amp|lt|gt|quot|#39);/g, (e) =>
      Object.keys(ENTITIES).find((c) => ENTITIES[c] === e)!,
    )
    .trim();

const slug = (text: string) =>
  text
    .toLowerCase()
    .replace(/[^\w\- ]+/g, "")
    .trim()
    .replace(/\s+/g, "-");

export const copyLabel = (done: boolean) =>
  `${iconMarkup(done ? "check" : "content_copy", 16)}<span>${done ? "Copied" : "Copy"}</span>`;

// Links between documents stay inside the site; other repository files go to GitHub.
function rewrite(href: string) {
  if (/^(https?:|mailto:|#)/.test(href)) return href;
  const m = href.match(/^(?:\.\/)?([A-Z_]+)\.md(?:#(.*))?$/);
  if (m && DOCS.some((d) => d.file === m[1])) return url(docPath(m[1], m[2]));
  return `https://github.com/${REPO}/blob/main/docs/${href}`;
}

type Callout = Tokens.Blockquote & { callout?: string };

const md = new Marked({ gfm: true });
md.use({
  renderer: {
    heading({ tokens, depth }) {
      const inner = this.parser.parseInline(tokens);
      const id = slug(plain(inner));
      return `<h${depth} id="${id}">${inner}<a class="anchor" href="#${id}" aria-label="Link to this section">${iconMarkup("link", 18)}</a></h${depth}>\n`;
    },
    code({ text, lang }) {
      const name = lang?.match(/^\S*/)?.[0] ?? "";
      return `<div class="code"><div class="code-bar"><span>${name && name !== "text" ? esc(name) : ""}</span><button type="button" class="code-copy">${copyLabel(false)}</button></div><pre><code${name ? ` class="language-${esc(name)}"` : ""}>${esc(text)}\n</code></pre></div>\n`;
    },
    blockquote(token) {
      const kind = (token as Callout).callout;
      if (!kind) return false;
      const [title, symbol] = CALLOUTS[kind];
      return `<div class="callout callout-${kind.toLowerCase()}"><p class="callout-title">${iconMarkup(symbol, 20)}${title}</p>${this.parser.parse(token.tokens)}</div>\n`;
    },
    link({ href, title, tokens }) {
      return `<a class="link" href="${esc(rewrite(href))}"${title ? ` title="${esc(title)}"` : ""}>${this.parser.parseInline(tokens)}</a>`;
    },
  },
});

function enhance(tokens: Token[]) {
  md.walkTokens(tokens, (token) => {
    if (token.type === "blockquote") {
      const marker = token.text.match(
        /^\s*\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*/,
      );
      if (!marker) return;
      (token as Callout).callout = marker[1];
      token.tokens = md.lexer(token.text.slice(marker[0].length));
    }
    if (token.type === "table") {
      const keys = (token as Tokens.Table).header.flatMap((cell, i) =>
        /key|alternative/i.test(cell.text) ? [i] : [],
      );
      for (const row of (token as Tokens.Table).rows)
        for (const i of keys)
          row[i].tokens = row[i].tokens.map((t) =>
            t.type === "codespan"
              ? {
                  type: "html",
                  raw: t.raw,
                  block: false,
                  pre: false,
                  text: t.text
                    .split(/\s*\+\s*/)
                    .map((k: string) => `<kbd>${esc(k)}</kbd>`)
                    .join('<span class="kbd-plus">+</span>'),
                }
              : t,
          );
    }
  });
}

export interface Doc {
  file: string;
  title: string;
  lead: string;
  html: string;
  headings: { id: string; text: string; sub: boolean }[];
}

const cache = new Map<string, Doc | null>();

export function render(file: string): Doc | null {
  if (cache.has(file)) return cache.get(file)!;
  const text = source(file);
  if (text === undefined) {
    cache.set(file, null);
    return null;
  }
  const tokens = md.lexer(text);
  const h1 = tokens.findIndex((t) => t.type === "heading" && t.depth === 1);
  const title =
    h1 < 0
      ? (DOCS.find((d) => d.file === file)?.title ?? file)
      : plain(
          md.parseInline((tokens[h1] as Tokens.Heading).text, { async: false }),
        );
  let lead = "";
  if (h1 >= 0) {
    let next = h1 + 1;
    while (tokens[next]?.type === "space") next++;
    if (tokens[next]?.type === "paragraph") {
      lead = md.parseInline((tokens[next] as Tokens.Paragraph).text, {
        async: false,
      });
      tokens.splice(next, 1);
    }
    tokens.splice(h1, 1);
  }
  enhance(tokens);
  const html = md
    .parser(tokens)
    .replace(/<table>/g, '<div class="table-frame"><table>')
    .replace(/<\/table>/g, "</table></div>");
  const headings = tokens.flatMap((t) => {
    if (t.type !== "heading" || (t.depth !== 2 && t.depth !== 3)) return [];
    const text = plain(
      md.parseInline((t as Tokens.Heading).text, { async: false }),
    );
    return [{ id: slug(text), text, sub: t.depth === 3 }];
  });
  const doc = { file, title, lead, html, headings };
  cache.set(file, doc);
  return doc;
}
