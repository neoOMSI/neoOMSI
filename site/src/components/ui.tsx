import { useMemo, type CSSProperties, type ReactNode } from "react";
import { REPO } from "../content/data";
import { untrusted, type Label } from "../lib/github";

export const PageHead = ({ children }: { children: ReactNode }) => (
  <header className="wrap pt-32 pb-12 sm:pt-44 sm:pb-16">{children}</header>
);

export const Tag = ({
  color,
  children,
}: {
  color: string;
  children: ReactNode;
}) => (
  <span className="label" style={{ "--lc": color } as CSSProperties}>
    {children}
  </span>
);

export const Labels = ({ list }: { list: Label[] }) => (
  <>
    {list.map((l) => (
      <Tag key={l.name} color={`#${l.color}`}>
        {l.name}
      </Tag>
    ))}
  </>
);

export function Untrusted({
  text,
  empty,
  className,
}: {
  text: string;
  empty: string;
  className: string;
}) {
  const html = useMemo(() => untrusted(text), [text]);
  if (!html)
    return (
      <div className={className}>
        <p className="text-muted">{empty}</p>
      </div>
    );
  return (
    <div className={className} dangerouslySetInnerHTML={{ __html: html }} />
  );
}

export const Failure = ({
  error,
  path,
  what,
}: {
  error: Error;
  path: string;
  what: string;
}) => (
  <p>
    {error.message} See the{" "}
    <a className="link" href={`https://github.com/${REPO}/${path}`}>
      {what} on GitHub
    </a>
    .
  </p>
);
