import { Fragment, useEffect, useState, type MouseEvent } from "react";
import { DOCS, REPO } from "../content/data";
import { copyLabel, render, type Doc } from "../content/docs";
import { Icon } from "../components/icons";
import { docPath, url } from "../lib/routes";

const GROUPS = [...new Set(DOCS.map((d) => d.group))];

async function copy(e: MouseEvent) {
  const button = (e.target as HTMLElement).closest<HTMLButtonElement>(
    ".code-copy",
  );
  if (!button) return;
  await navigator.clipboard.writeText(
    button.closest(".code")!.querySelector("pre")!.textContent || "",
  );
  button.innerHTML = copyLabel(true);
  setTimeout(() => (button.innerHTML = copyLabel(false)), 1600);
}

const DocsNav = ({ file }: { file: string }) =>
  GROUPS.map((group) => (
    <Fragment key={group}>
      <p className="mt-8 mb-2 px-3 text-[14px] font-semibold text-muted first:mt-0">
        {group}
      </p>
      <ul className="space-y-1">
        {DOCS.filter((d) => d.group === group).map((d) => (
          <li key={d.file}>
            <a
              href={url(docPath(d.file))}
              className={`docs-link${d.file === file ? " on" : ""}`}
              aria-current={d.file === file ? "page" : undefined}
            >
              <span className="truncate">{d.title}</span>
              <Icon name={d.icon} size={18} />
            </a>
          </li>
        ))}
      </ul>
    </Fragment>
  ));

function Contents({ headings }: { headings: Doc["headings"] }) {
  const [current, setCurrent] = useState(0);

  useEffect(() => {
    const spy = () => {
      let at = 0;
      headings.forEach((h, i) => {
        const el = document.getElementById(h.id);
        if (el && el.getBoundingClientRect().top < 140) at = i;
      });
      if (
        window.innerHeight + window.scrollY >=
        document.documentElement.scrollHeight - 4
      )
        at = headings.length - 1;
      setCurrent(at);
    };
    let raf = 0;
    const scroll = () => {
      if (!raf) raf = requestAnimationFrame(() => ((raf = 0), spy()));
    };
    spy();
    window.addEventListener("scroll", scroll, { passive: true });
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("scroll", scroll);
    };
  }, [headings]);

  if (headings.length < 2) return null;
  return (
    <>
      <p className="mb-4 text-[14px] font-semibold text-muted">On this page</p>
      <ol className="toc">
        {headings.map((h, i) => (
          <li
            key={h.id}
            className={[
              h.sub && "sub",
              i < current && "passed",
              i === current && "current",
            ]
              .filter(Boolean)
              .join(" ")}
          >
            <a href={`#${h.id}`}>{h.text}</a>
          </li>
        ))}
      </ol>
    </>
  );
}

function Neighbour({
  doc,
  label,
  next,
}: {
  doc?: (typeof DOCS)[number];
  label: string;
  next?: boolean;
}) {
  if (!doc) return null;
  return (
    <a
      href={url(docPath(doc.file))}
      rel={next ? "next" : "prev"}
      className={`group rounded-lg border border-line px-5 py-4 transition-colors hover:border-brand${next ? " sm:col-start-2 sm:text-right" : ""}`}
    >
      <span className="block text-[14px] text-muted">{label}</span>
      <span className="mt-0.5 block font-semibold text-heading group-hover:text-accent">
        {doc.title}
      </span>
    </a>
  );
}

export function Docs({ file }: { file: string }) {
  const doc = render(file)!;
  const index = DOCS.findIndex((d) => d.file === file);

  return (
    <>
      <header className="wrap-wide pt-32 pb-12 sm:pt-40 sm:pb-14 lg:pl-[calc(2.5rem+15rem+3rem)]">
        <nav
          aria-label="Breadcrumb"
          className="mb-4 flex gap-2 text-[15px] text-muted"
        >
          <a className="hover:text-ink" href={url("/docs/user-guide/")}>
            Docs
          </a>
          <span aria-hidden="true">/</span>
          <span>{DOCS[index].group}</span>
        </nav>
        <h1 className="display max-w-[16em] text-[clamp(2.25rem,4.6vw,3.5rem)]">
          {doc.title}
        </h1>
        {doc.lead && (
          <p
            className="mt-6 max-w-[38em] text-[19px] leading-relaxed text-muted"
            dangerouslySetInnerHTML={{ __html: doc.lead }}
          />
        )}
      </header>
      <div className="wrap-wide grid grid-cols-[minmax(0,1fr)] gap-x-12 gap-y-6 pt-0 pb-20 lg:grid-cols-[15rem_minmax(0,1fr)] xl:grid-cols-[15rem_minmax(0,1fr)_14rem]">
        <details key={file} className="rounded-lg border border-line lg:hidden">
          <summary className="flex cursor-pointer list-none items-center gap-3 px-4 py-3 font-medium [&::-webkit-details-marker]:hidden">
            <Icon name="menu" size={20} />
            <span>{DOCS[index].title}</span>
            <span className="ml-auto text-muted">
              <Icon name="expand_more" size={20} />
            </span>
          </summary>
          <nav
            aria-label="Documentation"
            className="docs-nav border-t border-line px-1 py-4"
          >
            <DocsNav file={file} />
          </nav>
        </details>
        <aside className="hidden lg:block">
          <nav aria-label="Documentation" className="docs-nav sticky top-24">
            <DocsNav file={file} />
          </nav>
        </aside>
        <div className="min-w-0">
          <article
            className="doc"
            onClick={copy}
            dangerouslySetInnerHTML={{ __html: doc.html }}
          />
          <div className="mt-16 grid gap-3 sm:grid-cols-2">
            <Neighbour doc={DOCS[index - 1]} label="Previous" />
            <Neighbour doc={DOCS[index + 1]} label="Next" next />
          </div>
          <p className="mt-8 flex items-center gap-2 text-[15px] text-muted">
            <Icon name="open_in_new" size={16} />
            <a
              className="hover:text-ink"
              href={`https://github.com/${REPO}/blob/main/docs/${file}.md`}
            >
              Edit this page on GitHub
            </a>
          </p>
        </div>
        <aside className="hidden xl:block">
          <nav className="sticky top-24" aria-label="On this page">
            <Contents headings={doc.headings} />
          </nav>
        </aside>
      </div>
    </>
  );
}
