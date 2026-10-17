import { useEffect, useState, type CSSProperties } from "react";
import { Failure, Labels, PageHead, Untrusted } from "../components/ui";
import { REPO } from "../content/data";
import {
  ago,
  gh,
  type Comment,
  type Issue as IssueData,
  type User,
} from "../lib/github";
import { useAsync, useTitle } from "../lib/hooks";
import { docPath, url } from "../lib/routes";
import { Icon } from "../components/icons";

const OPEN = "#3fb950";
const CLOSED = "#a371f7";
const STATES = ["open", "closed", "all"];

const filters = { state: "open", query: "", label: "" };

const StateIcon = ({ state }: { state: string }) =>
  state === "open" ? (
    <svg
      viewBox="0 0 16 16"
      aria-label="Open"
      style={{ width: 20, height: 20, flex: "none", color: OPEN }}
    >
      <circle
        cx="8"
        cy="8"
        r="6.25"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      />
      <circle cx="8" cy="8" r="1.75" fill="currentColor" />
    </svg>
  ) : (
    <span aria-label="Closed" style={{ color: CLOSED, display: "inline-flex" }}>
      <Icon name="check_circle" size={20} />
    </span>
  );

function IssueList({ list, state }: { list: IssueData[]; state: string }) {
  if (!list.length)
    return (
      <div className="rounded-xl border border-dashed border-line px-6 py-12 text-center">
        <p className="font-semibold text-heading">
          No {state === "all" ? "" : `${state} `}issues match.
        </p>
        <p className="mt-1 text-muted">
          Try another filter, or{" "}
          <a className="link" href={`https://github.com/${REPO}/issues/new`}>
            report a new issue
          </a>
          .
        </p>
      </div>
    );
  return (
    <>
      <p className="mb-3 text-[15px] text-muted">
        {list.length} {list.length === 1 ? "issue" : "issues"}
      </p>
      <ul className="overflow-hidden rounded-xl border border-line">
        {list.map((i) => (
          <li key={i.number} className="border-t border-line first:border-t-0">
            <a
              href={url(`/issues/${i.number}`)}
              className="group flex gap-4 px-4 py-4 transition-colors hover:bg-sunken sm:px-5"
            >
              <span className="mt-[3px]">
                <StateIcon state={i.state} />
              </span>
              <span className="min-w-0 flex-1">
                <span className="font-semibold text-heading [overflow-wrap:anywhere] group-hover:text-accent">
                  {i.title}
                </span>{" "}
                <Labels list={i.labels} />
                <span className="mt-0.5 block text-[15px] text-muted">
                  #{i.number} opened by {i.user?.login}, updated{" "}
                  {ago(i.updated_at)}
                </span>
              </span>
              {i.comments > 0 && (
                <span
                  className="mt-0.5 flex shrink-0 items-start gap-1.5 text-[15px] text-muted"
                  title={`${i.comments} comments`}
                >
                  <Icon name="chat" size={18} style={{ marginTop: 3 }} />
                  {i.comments}
                </span>
              )}
            </a>
          </li>
        ))}
      </ul>
    </>
  );
}

export function Issues() {
  const [state, setState] = useState(filters.state);
  const [query, setQuery] = useState(filters.query);
  const [label, setLabel] = useState(filters.label);
  const { data: list, error } = useAsync(
    async () =>
      (
        await gh<IssueData[]>(`issues?state=${state}&per_page=100&sort=updated`)
      ).filter((i) => !i.pull_request),
    [state],
  );

  useEffect(() => {
    Object.assign(filters, { state, query, label });
  }, [state, query, label]);

  const all = list?.flatMap((i) => i.labels) ?? [];
  const chosen = all.some((l) => l.name === label) ? label : "";
  const names = [...new Set(all.map((l) => l.name))].sort();
  const q = query.trim().toLowerCase();
  const shown = list?.filter(
    (i) =>
      (!chosen || i.labels.some((l) => l.name === chosen)) &&
      (!q ||
        i.title.toLowerCase().includes(q) ||
        i.labels.some((l) => l.name.toLowerCase().includes(q)) ||
        String(i.number) === q.replace("#", "")),
  );

  return (
    <>
      <PageHead>
        <h1 className="display">Issues</h1>
        <p className="mt-6 max-w-[34em] text-[19px] text-muted">
          Report bugs, compatibility problems and feature requests on GitHub.
          A GitHub account is required. For bugs, include your build, map, bus
          and steps to reproduce the problem.
        </p>
        <div className="mt-8 flex flex-wrap items-center gap-x-6 gap-y-3">
          <a className="btn" href={`https://github.com/${REPO}/issues/new`}>
            Report an issue
          </a>
          <a className="link text-[16px]" href={url(docPath("ISSUE_TRIAGE"))}>
            How issues are handled
          </a>
        </div>
      </PageHead>
      <div className="wrap pb-20 sm:pb-24">
        <div className="flex flex-wrap items-center gap-3">
          <div
            className="inline-flex rounded-lg border border-line p-1"
            role="group"
            aria-label="State"
          >
            {STATES.map((s) => (
              <button
                key={s}
                type="button"
                className="seg capitalize"
                aria-pressed={s === state}
                onClick={() => setState(s)}
              >
                {s}
              </button>
            ))}
          </div>
          <label className="relative min-w-[14rem] flex-1">
            <span className="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-muted">
              <Icon name="search" size={20} />
            </span>
            <input
              type="search"
              placeholder="Filter by title, label or number"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="w-full rounded-lg border border-line bg-page py-2.5 pr-4 pl-11 outline-none transition-colors focus:border-brand"
            />
          </label>
        </div>
        <div className="mt-4 flex flex-wrap gap-2">
          {names.map((name) => (
            <button
              key={name}
              type="button"
              aria-pressed={name === chosen}
              onClick={() => setLabel(name === chosen ? "" : name)}
              className="label-filter"
              style={
                {
                  "--lc": `#${all.find((l) => l.name === name)!.color}`,
                } as CSSProperties
              }
            >
              {name}
            </button>
          ))}
        </div>
        <div className="mt-6">
          {error ? (
            <Failure error={error} path="issues" what="issues" />
          ) : shown ? (
            <IssueList list={shown} state={state} />
          ) : (
            <p className="text-muted">Loading issues…</p>
          )}
        </div>
      </div>
    </>
  );
}

function Post({
  who,
  when,
  body,
  author,
}: {
  who: User;
  when: string;
  body: string;
  author: boolean;
}) {
  return (
    <li className="post">
      <img className="post-avatar" src={`${who?.avatar_url}&s=80`} alt="" />
      <div
        className={`min-w-0 flex-1 overflow-hidden rounded-xl border ${author ? "border-brand/40" : "border-line"}`}
      >
        <div
          className={`flex flex-wrap items-center gap-x-2 border-b px-4 py-2.5 text-[15px] ${author ? "border-brand/30 bg-brand/[.06]" : "border-line bg-sunken"}`}
        >
          <b className="text-heading">{who?.login}</b>
          <span className="text-muted">{ago(when)}</span>
          {author && (
            <span className="ml-auto rounded-full border border-brand/40 px-2 text-[13px] text-accent">
              Author
            </span>
          )}
        </div>
        <Untrusted
          className="doc doc-plain max-w-none px-4 py-4 [overflow-wrap:anywhere] sm:px-5"
          text={body}
          empty="No description."
        />
      </div>
    </li>
  );
}

export function Issue({ n }: { n: string }) {
  const { data, error } = useAsync(
    () =>
      Promise.all([
        gh<IssueData>(`issues/${n}`),
        gh<Comment[]>(`issues/${n}/comments?per_page=100`),
      ]),
    [n],
  );
  useTitle(data && `${data[0].title} (#${data[0].number})`);
  const heading = "display text-[clamp(1.9rem,3.6vw,2.9rem)]";

  if (error)
    return (
      <>
        <PageHead>
          <h1 className={heading}>Issue #{n}</h1>
        </PageHead>
        <div className="wrap pb-20 sm:pb-24">
          <Failure error={error} path={`issues/${n}`} what="issue" />
        </div>
      </>
    );
  if (!data)
    return (
      <PageHead>
        <p className="text-muted">Loading the issue…</p>
      </PageHead>
    );

  const [i, comments] = data;
  const open = i.state === "open";
  const people = [
    ...new Map(
      [i.user, ...comments.map((c) => c.user)]
        .filter(Boolean)
        .map((u) => [u.login, u]),
    ).values(),
  ];
  return (
    <>
      <PageHead>
        <h1 className={`${heading} max-w-[22em] [overflow-wrap:anywhere]`}>
          {i.title} <span className="text-muted">#{i.number}</span>
        </h1>
        <p className="mt-6 flex flex-wrap items-center gap-x-3 gap-y-2 text-muted">
          <span
            className="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-[15px] font-semibold text-heading"
            style={{ background: open ? OPEN : CLOSED }}
          >
            {open ? "Open" : "Closed"}
          </span>
          <span>
            <b className="text-ink">{i.user?.login}</b> opened this{" "}
            {ago(i.created_at)}, {comments.length}{" "}
            {comments.length === 1 ? "comment" : "comments"}
          </span>
        </p>
      </PageHead>
      <div className="wrap pb-20 sm:pb-24">
        <div className="grid gap-x-12 gap-y-10 lg:grid-cols-[minmax(0,1fr)_15rem]">
          <ol className="thread">
            <Post who={i.user} when={i.created_at} body={i.body} author />
            {comments.map((c, k) => (
              <Post
                key={k}
                who={c.user}
                when={c.created_at}
                body={c.body}
                author={c.user?.login === i.user?.login}
              />
            ))}
          </ol>
          <aside className="space-y-8 text-[15px] lg:sticky lg:top-24 lg:self-start">
            <div>
              <p className="mb-2 font-semibold text-muted">Labels</p>
              <p className="flex flex-wrap gap-1.5">
                {i.labels.length ? (
                  <Labels list={i.labels} />
                ) : (
                  <span className="text-muted">None yet</span>
                )}
              </p>
            </div>
            <div>
              <p className="mb-2 font-semibold text-muted">
                {people.length === 1
                  ? "Participant"
                  : `${people.length} participants`}
              </p>
              <p className="flex flex-wrap gap-1.5">
                {people.map((u) => (
                  <img
                    key={u.login}
                    className="size-8 rounded-full"
                    src={`${u.avatar_url}&s=64`}
                    alt={u.login}
                    title={u.login}
                  />
                ))}
              </p>
            </div>
            <a className="btn w-full justify-center gap-2" href={i.html_url}>
              <Icon name="open_in_new" size={18} />
              Comment on GitHub
            </a>
            <a
              className="flex items-center gap-1.5 text-muted hover:text-ink"
              href={url("/issues/")}
            >
              <Icon name="arrow_back" size={18} />
              All issues
            </a>
          </aside>
        </div>
      </div>
    </>
  );
}
