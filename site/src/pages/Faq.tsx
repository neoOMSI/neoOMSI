import { useState } from "react";
import { PageHead } from "../components/ui";
import { DISCORD, REPO } from "../content/data";
import { FAQ, type Question } from "../content/faq";
import { Icon } from "../components/icons";
import { url } from "../lib/routes";

// Collapsed answers are squeezed to zero height rather than removed, so crawlers still read them.
export function Answers({
  list,
  level = 2,
}: {
  list: Question[];
  level?: 2 | 3;
}) {
  const [open, setOpen] = useState(() => new Set([0]));
  const Heading = level === 2 ? "h2" : "h3";
  const toggle = (i: number) =>
    setOpen((now) => {
      const next = new Set(now);
      if (!next.delete(i)) next.add(i);
      return next;
    });
  return (
    <dl className="divide-y divide-line border-y border-line">
      {list.map(({ q, a }, i) => {
        const shown = open.has(i);
        return (
          <div key={q} className="py-6">
            <dt>
              <Heading className="text-[1.2rem] leading-snug">
                <button
                  type="button"
                  onClick={() => toggle(i)}
                  aria-expanded={shown}
                  className="group flex w-full items-start justify-between gap-4 text-left"
                >
                  <span className="group-hover:text-accent">{q}</span>
                  <span
                    className={`mt-0.5 text-muted transition-transform ${shown ? "rotate-180" : ""}`}
                  >
                    <Icon name="expand_more" size={22} />
                  </span>
                </button>
              </Heading>
            </dt>
            <dd
              className={`grid transition-[grid-template-rows] duration-200 ${shown ? "grid-rows-[1fr]" : "grid-rows-[0fr]"}`}
            >
              <p className="max-w-[44em] overflow-hidden text-muted">
                <span className="block pt-2">{a}</span>
              </p>
            </dd>
          </div>
        );
      })}
    </dl>
  );
}

export function Faq() {
  return (
    <>
      <PageHead>
        <h1 className="display">Questions and answers</h1>
        <p className="mt-6 max-w-[36em] text-[19px] text-muted">
          What neoOMSI is, what you need to play it, and how it runs OMSI&nbsp;2
          content on Windows, macOS, Linux and Android.
        </p>
      </PageHead>
      <div className="wrap grid gap-x-16 gap-y-10 pb-20 sm:pb-24 lg:grid-cols-[minmax(0,1fr)_minmax(0,2.4fr)]">
        <aside className="lg:sticky lg:top-24 lg:self-start">
          <p className="text-[16px] text-muted">
            Something missing? Ask on{" "}
            <a className="link" href={DISCORD}>
              Discord
            </a>{" "}
            or{" "}
            <a className="link" href={`https://github.com/${REPO}/issues/new`}>
              open an issue
            </a>
            . Coming from openOMSI? See{" "}
            <a className="link" href={url("/openomsi/")}>
              neoOMSI vs openOMSI
            </a>
            .
          </p>
        </aside>
        <Answers list={FAQ} />
      </div>
    </>
  );
}
