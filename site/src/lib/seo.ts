import mark from "../../../assets/icons/app/neoomsi-512.png";
import { DISCORD, DOCS, OPENOMSI, REPO, SITE } from "../content/data";
import { esc, plain, render } from "../content/docs";
import { FAQ, OPENOMSI_FAQ, type Question } from "../content/faq";
import { absolute } from "./routes";

export interface Meta {
  title: string;
  description: string;
  path: string;
  noindex?: boolean;
  type?: "website" | "article";
  schema?: object[];
}

export const OG_IMAGE = { path: "og.png", width: 1200, height: 630 };

export const DESCRIPTION =
  "neoOMSI is a free, open-source reimplementation of OMSI 2 for modern 64-bit systems. It uses maps, buses and other game files from your OMSI 2 installation.";

const ORG = {
  "@type": "Organization",
  "@id": `${SITE}#org`,
  name: "neoOMSI",
  url: SITE,
  logo: new URL(mark, SITE).href,
  sameAs: [`https://github.com/${REPO}`, DISCORD],
};

const WEBSITE = {
  "@type": "WebSite",
  "@id": `${SITE}#website`,
  name: "neoOMSI",
  url: SITE,
  inLanguage: "en",
  publisher: { "@id": ORG["@id"] },
};

const APP = {
  "@type": "SoftwareApplication",
  "@id": `${SITE}#app`,
  name: "neoOMSI",
  description: DESCRIPTION,
  url: SITE,
  downloadUrl: absolute("/download/"),
  image: absolute(OG_IMAGE.path),
  applicationCategory: "GameApplication",
  applicationSubCategory: "Bus simulator",
  operatingSystem: "Windows 10, Windows 11, macOS 11, Linux",
  softwareRequirements: "An installed copy of OMSI 2",
  programmingLanguage: "Rust",
  isAccessibleForFree: true,
  license: "https://www.gnu.org/licenses/gpl-3.0.html",
  offers: { "@type": "Offer", price: "0", priceCurrency: "EUR" },
  publisher: { "@id": ORG["@id"] },
  isBasedOn: {
    "@type": "SoftwareSourceCode",
    name: "openOMSI",
    codeRepository: OPENOMSI.repo,
    license: "https://opensource.org/license/mit",
  },
};

const faqPage = (list: Question[]) => ({
  "@type": "FAQPage",
  mainEntity: list.map(({ q, a }) => ({
    "@type": "Question",
    name: q,
    acceptedAnswer: { "@type": "Answer", text: a },
  })),
});

const crumbs = (...items: [string, string][]) => ({
  "@type": "BreadcrumbList",
  itemListElement: [["neoOMSI", "/"] as [string, string], ...items].map(
    ([name, path], i) => ({
      "@type": "ListItem",
      position: i + 1,
      name,
      item: absolute(path),
    }),
  ),
});

const clip = (text: string, n = 158) =>
  text.length <= n ? text : `${text.slice(0, text.lastIndexOf(" ", n - 3))}...`;

const STATIC: Record<string, Omit<Meta, "path">> = {
  "/": {
    title: "neoOMSI: OMSI 2 rebuilt for modern systems",
    description: DESCRIPTION,
    schema: [APP],
  },
  "/download": {
    title: "Download neoOMSI for Windows, macOS and Linux",
    description:
      "Download neoOMSI for Windows, macOS or Linux, including development builds and dedicated server packages. Requires your own OMSI 2 files.",
    schema: [APP, crumbs(["Download", "/download/"])],
  },
  "/releases": {
    title: "neoOMSI releases and changelog",
    description:
      "All neoOMSI releases and development changelogs, newest first, with packaged downloads for Windows, macOS, Linux and the dedicated server.",
    schema: [crumbs(["Releases", "/releases/"])],
  },
  "/issues": {
    title: "neoOMSI issues: bugs and feature requests",
    description:
      "Open and closed neoOMSI bug reports and feature requests, tracked on GitHub. See current issues or report a problem with a map, bus or mod.",
    schema: [crumbs(["Issues", "/issues/"])],
  },
  "/faq": {
    title: "neoOMSI FAQ: compatibility, installation and supported systems",
    description:
      "Answers about neoOMSI: OMSI 2 requirements, map and bus compatibility, supported systems, mods, multiplayer and project status.",
    schema: [faqPage(FAQ), crumbs(["FAQ", "/faq/"])],
  },
  "/openomsi": {
    title: "neoOMSI and openOMSI: how the two projects compare",
    description:
      "Learn how neoOMSI and openOMSI relate, their shared origins, development processes, and how to test neoOMSI alongside openOMSI.",
    type: "article",
    schema: [
      faqPage(OPENOMSI_FAQ),
      crumbs(["neoOMSI and openOMSI", "/openomsi/"]),
    ],
  },
};

export const PAGES = [
  ...Object.keys(STATIC),
  ...DOCS.map((d) => `/docs/${d.slug}`),
];

export function meta(path: string): Meta {
  const page = STATIC[path];
  if (page) return { ...page, path };

  const slug = path.match(/^\/docs\/([\w-]+)$/)?.[1];
  const entry = DOCS.find((d) => d.slug === slug);
  const doc = entry && render(entry.file);
  if (entry && doc) {
    const description = clip(
      plain(doc.lead) ||
        `${doc.title} for neoOMSI, the open-source OMSI 2 recreation in Rust.`,
    );
    return {
      title: `${doc.title}: neoOMSI docs`,
      description,
      path,
      type: "article",
      schema: [
        {
          "@type": "TechArticle",
          headline: doc.title,
          description,
          url: absolute(`${path}/`),
          image: absolute(OG_IMAGE.path),
          inLanguage: "en",
          isPartOf: { "@id": WEBSITE["@id"] },
          publisher: { "@id": ORG["@id"] },
          about: { "@id": APP["@id"] },
        },
        crumbs(["Docs", "/docs/user-guide/"], [doc.title, `${path}/`]),
      ],
    };
  }

  const issue = path.match(/^\/issues\/(\d+)$/)?.[1];
  if (issue)
    return {
      title: `Issue #${issue} | neoOMSI`,
      description: STATIC["/issues"].description,
      path,
      noindex: true,
    };

  return {
    title: "Page not found | neoOMSI",
    description: DESCRIPTION,
    path,
    noindex: true,
  };
}

const canonical = (path: string) =>
  absolute(path === "/" || /\/\d+$/.test(path) ? path : `${path}/`);

const json = (value: unknown) => JSON.stringify(value).replace(/</g, "\\u003c");

export function headTags(m: Meta) {
  const url = canonical(m.path);
  const image = absolute(OG_IMAGE.path);
  const tags = [
    `<title>${esc(m.title)}</title>`,
    `<meta name="description" content="${esc(m.description)}">`,
    `<meta name="robots" content="${m.noindex ? "noindex, follow" : "index, follow, max-image-preview:large, max-snippet:-1"}">`,
    `<meta property="og:type" content="${m.type ?? "website"}">`,
    '<meta property="og:site_name" content="neoOMSI">',
    '<meta property="og:locale" content="en_US">',
    `<meta property="og:title" content="${esc(m.title)}">`,
    `<meta property="og:description" content="${esc(m.description)}">`,
    `<meta property="og:url" content="${url}">`,
    `<meta property="og:image" content="${image}">`,
    `<meta property="og:image:width" content="${OG_IMAGE.width}">`,
    `<meta property="og:image:height" content="${OG_IMAGE.height}">`,
    '<meta property="og:image:alt" content="neoOMSI, OMSI 2 recreated in Rust">',
    '<meta name="twitter:card" content="summary_large_image">',
    `<meta name="twitter:title" content="${esc(m.title)}">`,
    `<meta name="twitter:description" content="${esc(m.description)}">`,
    `<meta name="twitter:image" content="${image}">`,
  ];
  if (!m.noindex) tags.splice(3, 0, `<link rel="canonical" href="${url}">`);
  if (!m.noindex)
    tags.push(
      `<script type="application/ld+json">${json({ "@context": "https://schema.org", "@graph": [ORG, WEBSITE, ...(m.schema ?? [])] })}</script>`,
    );
  return tags.map((t) => t.replace(/^<(\w+)/, "<$1 data-head")).join("\n");
}
