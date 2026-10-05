import { Failure, PageHead, Tag, Untrusted } from "../components/ui";
import { DISCORD, PLATFORMS, REPO, SERVERS, type Build } from "../content/data";
import {
  ago,
  date,
  gh,
  megabytes,
  version,
  type Asset,
  type Release,
} from "../lib/github";
import { useAsync } from "../lib/hooks";
import { url } from "../lib/routes";
import { Icon, PlatformIcon } from "../components/icons";

const BUILDS = [...PLATFORMS, ...SERVERS];

const buildOf = (asset: Asset) =>
  [...SERVERS, ...PLATFORMS].find((b) =>
    asset.name.endsWith(`-${b.key}.${b.ext || "zip"}`),
  );

function File({ asset, build }: { asset: Asset; build?: Build }) {
  return (
    <li>
      <a className="file" href={asset.browser_download_url}>
        <span className="text-heading">
          <PlatformIcon build={build?.key} size={20} />
        </span>
        <span className="min-w-0 flex-1 truncate font-medium text-heading">
          {!build
            ? asset.name
            : SERVERS.includes(build)
              ? `${build.family} ${build.arch}`
              : build.name}
        </span>
        <span className="file-get flex shrink-0 items-center gap-1.5 text-[14px] text-muted">
          {megabytes(asset.size)}
          <Icon name="download" size={18} />
        </span>
      </a>
    </li>
  );
}

function Files({ assets }: { assets: Asset[] }) {
  const known = assets
    .map((asset) => ({ asset, build: buildOf(asset) }))
    .sort(
      (a, b) =>
        (a.build ? BUILDS.indexOf(a.build) : BUILDS.length) -
        (b.build ? BUILDS.indexOf(b.build) : BUILDS.length),
    );
  const groups: [string, typeof known][] = [
    ["Game", known.filter((f) => f.build && PLATFORMS.includes(f.build))],
    [
      "Dedicated server",
      known.filter((f) => f.build && SERVERS.includes(f.build)),
    ],
    ["Other files", known.filter((f) => !f.build)],
  ];
  return (
    <div className="card mt-6 grid gap-x-8 gap-y-4 p-3 sm:p-4">
      {groups.map(
        ([title, files]) =>
          files.length > 0 && (
            <div key={title}>
              <h3 className="px-3 pb-1 text-[14px] font-semibold text-muted">
                {title}
              </h3>
              <ul className="grid gap-x-2 sm:grid-cols-2 xl:grid-cols-3">
                {files.map(({ asset, build }) => (
                  <File key={asset.name} asset={asset} build={build} />
                ))}
              </ul>
            </div>
          ),
      )}
    </div>
  );
}

function Entry({ release: r, index }: { release: Release; index: number }) {
  return (
    <li className={index === 0 ? "latest" : ""}>
      <p className="when">
        {date(r.published_at)}
        <span>{ago(r.published_at)}</span>
      </p>
      <div className="stop">
        <details className="group" open={index < 2}>
          <summary className="flex cursor-pointer list-none flex-wrap items-baseline gap-x-3 gap-y-1 [&::-webkit-details-marker]:hidden">
            <span className="text-[1.6rem] leading-tight font-semibold text-heading">
              {version(r)}
            </span>
            {index === 0 && !r.prerelease && <Tag color="#2da44e">Latest</Tag>}
            {r.prerelease && <Tag color="#d8a020">Pre-release</Tag>}
            <span className="text-muted">
              {r.name && r.name !== r.tag_name ? r.name : ""}
            </span>
            <span className="ml-auto text-[15px] text-muted group-open:hidden">
              Show notes
            </span>
          </summary>
          <div className="pt-4">
            <Untrusted
              className="doc doc-plain max-w-none"
              text={r.body}
              empty="No notes."
            />
            {r.assets.length > 0 && <Files assets={r.assets} />}
          </div>
        </details>
      </div>
    </li>
  );
}

export function Releases() {
  const { data: list, error } = useAsync(
    () => gh<Release[]>("releases?per_page=30"),
    [],
  );

  return (
    <>
      <PageHead>
        <h1 className="display">Releases</h1>
        <p className="mt-6 max-w-[34em] text-[19px] text-muted">
          Every version and what changed, newest first. The newest one is also
          on the{" "}
          <a className="link" href={url("/download/")}>
            download page
          </a>
          .
        </p>
      </PageHead>
      <div className="wrap pb-20 sm:pb-24">
        {error ? (
          <Failure error={error} path="releases" what="releases" />
        ) : !list ? (
          <p className="text-muted">Loading releases…</p>
        ) : list.length ? (
          <ol className="timeline">
            {list.map((r, i) => (
              <Entry key={r.tag_name} release={r} index={i} />
            ))}
          </ol>
        ) : (
          <>
            <p className="max-w-[34em] text-[18px]">
              No release has been published yet.
            </p>
            <p className="mt-2 max-w-[34em] text-muted">
              Follow along on{" "}
              <a className="link" href={`https://github.com/${REPO}`}>
                GitHub
              </a>{" "}
              or{" "}
              <a className="link" href={DISCORD}>
                Discord
              </a>{" "}
              to hear when the first one is out.
            </p>
          </>
        )}
      </div>
    </>
  );
}
