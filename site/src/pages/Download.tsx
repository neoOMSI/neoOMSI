import { useState, type ReactNode } from "react";
import { PageHead, Tag } from "../components/ui";
import { PLATFORMS, SERVERS, visitorBuild, type Build } from "../content/data";
import {
  date,
  megabytes,
  releases,
  version,
  type Release,
} from "../lib/github";
import { useAsync } from "../lib/hooks";
import { docPath, url } from "../lib/routes";
import { Icon, PlatformIcon } from "../components/icons";

const INSTALL: Record<string, ReactNode> = {
  Windows: (
    <>
      Unpack the zip into a folder of your own (not <code>Program Files</code>)
      and run <code>neoomsi.exe</code>. SmartScreen may warn about an unknown
      app: choose <i>More info</i>, then <i>Run anyway</i>.
    </>
  ),
  macOS: (
    <>
      Unpack and open <code>neoOMSI.app</code>. If macOS refuses an app from the
      internet, right-click it and choose <i>Open</i> twice, or run{" "}
      <code>xattr -dr com.apple.quarantine neoOMSI.app</code> once.
    </>
  ),
  Linux: (
    <>
      Unpack and run <code>./neoomsi</code>. If it does not start, run{" "}
      <code>chmod +x neoomsi</code> first. Vulkan or OpenGL drivers are needed.
    </>
  ),
};

const TABS = Object.keys(INSTALL);
const DOWNLOAD_PLATFORMS = PLATFORMS.filter(
  (build) => build.family !== "Android",
);

const asset = (release: Release | null | undefined, build: Build) => {
  const file =
    release && `neoOMSI-${version(release)}-${build.key}.${build.ext || "zip"}`;
  return release?.assets.find((a) => a.name === file);
};

const CHANNELS = [
  { key: "stable", name: "Stable", icon: "check_circle" },
  { key: "rc", name: "Release candidate", icon: "flag" },
  { key: "nightly", name: "Nightly", icon: "nights_stay" },
] as const;

type Channel = (typeof CHANNELS)[number]["key"];

const releaseChannel = (release: Release): Channel | undefined => {
  if (!release.prerelease) return "stable";
  if (/-nightly\./i.test(release.tag_name)) return "nightly";
  if (/-rc\.\d+$/i.test(release.tag_name)) return "rc";
};

const newestRelease = (list: Release[], channel: Channel) =>
  list
    .filter((release) => releaseChannel(release) === channel)
    .sort(
      (a, b) => Date.parse(b.published_at) - Date.parse(a.published_at),
    )[0] ?? null;

const families = (builds: Build[]) =>
  [...new Set(builds.map((b) => b.family))].map((family) =>
    builds.filter((b) => b.family === family),
  );

function BuildRow({
  build,
  release,
}: {
  build: Build;
  release?: Release | null;
}) {
  const file = asset(release, build);
  const body = (
    <>
      <span className="min-w-0 flex-1">
        <span className="block font-semibold text-heading">{build.arch}</span>
        <span className="block text-[14.5px] leading-snug text-muted">
          {build.note}
        </span>
      </span>
      {file ? (
        <span className="file-get flex shrink-0 items-center gap-1.5 text-[14px] text-muted">
          {megabytes(file.size)}
          <Icon name="download" size={20} />
        </span>
      ) : (
        <span className="shrink-0 text-[13.5px] text-muted">
          Not in this release
        </span>
      )}
    </>
  );
  return (
    <li>
      {file ? (
        <a
          className="file -mx-3"
          href={file.browser_download_url}
          download
          aria-label={`Download ${build.name}, ${megabytes(file.size)}`}
        >
          {body}
        </a>
      ) : (
        <div className="-mx-3 flex items-center gap-3 px-3 py-2.5">{body}</div>
      )}
    </li>
  );
}

function Tiles({
  builds,
  release,
  mine,
}: {
  builds: Build[];
  release?: Release | null;
  mine?: Build;
}) {
  return families(builds).map((group) => {
    const yours = group[0].family === mine?.family;
    return (
      <div key={group[0].family} className="card flex flex-col gap-3 p-5">
        <div className="flex items-center gap-3 text-heading">
          <span className={yours ? "text-accent" : ""}>
            <PlatformIcon build={group[0].key} size={26} />
          </span>
          <h3 className="text-[1.2rem]">{group[0].family}</h3>
          {yours && (
            <span className="ml-auto rounded-full bg-line px-2.5 py-0.5 text-[13.5px] font-medium text-accent">
              Your system
            </span>
          )}
        </div>
        <ul className="space-y-1">
          {group.map((build) => (
            <BuildRow key={build.key} build={build} release={release} />
          ))}
        </ul>
      </div>
    );
  });
}

const jump = () =>
  document.getElementById("dl-all")!.scrollIntoView({ behavior: "smooth" });

export function Download() {
  const mine = DOWNLOAD_PLATFORMS.find(
    (build) => build.key === visitorBuild()?.key,
  );
  const [tab, setTab] = useState(() =>
    TABS.includes(mine?.family ?? "") ? mine!.family : TABS[0],
  );
  const { data: releaseList, error: releaseError } = useAsync(releases, []);
  const [channel, setChannel] = useState<Channel | null>(null);
  const available = CHANNELS.flatMap((entry) => {
    const release = releaseList && newestRelease(releaseList, entry.key);
    return release ? [{ ...entry, release }] : [];
  });
  const selected =
    available.find((entry) => entry.key === channel) ?? available[0];
  const release = selected?.release;
  const stable = available.find((entry) => entry.key === "stable");
  const file = mine && asset(release, mine);

  return (
    <>
      <PageHead>
        <h1 className="display">Download</h1>
        <p className="mt-6 max-w-[38em] text-[19px] text-muted">
          Play your OMSI 2 maps and buses on Windows, macOS or Linux.
        </p>
        {available.length > 1 && (
          <div
            className="mt-8 flex flex-wrap gap-3"
            role="group"
            aria-label="Build channel"
          >
            {available.map((entry) => (
              <button
                key={entry.key}
                type="button"
                className={`${selected === entry ? "btn" : "btn-quiet"} gap-2`}
                aria-pressed={selected === entry}
                onClick={() => setChannel(entry.key)}
              >
                <Icon name={entry.icon} size={18} />
                {entry.name}
              </button>
            ))}
          </div>
        )}
        {release && (
          <div className="mt-8 max-w-[42em]">
            <div className="flex flex-wrap items-center gap-3">
              <Tag color={release.prerelease ? "#d8a020" : "#2da44e"}>
                <span className="inline-flex items-center gap-1.5">
                  <Icon name={selected!.icon} size={15} />
                  {selected!.name}
                </span>
              </Tag>
              <span className="text-muted">
                {version(release)} · {date(release.published_at)}
              </span>
              <a className="link" href={release.html_url}>
                Release notes
              </a>
            </div>
            {release.prerelease && (
              <div className="card mt-4 flex gap-3 p-4">
                <span className="shrink-0 text-accent">
                  <Icon name="info" size={22} />
                </span>
                <div className="min-w-0 text-[16px]">
                  <p className="font-semibold text-heading">
                    {selected?.key === "rc"
                      ? "Release candidate"
                      : "Development build"}
                  </p>
                  <p className="mt-1 text-muted">
                    {selected?.key === "rc"
                      ? "Preview of the next stable release. Bugs may still occur."
                      : "Includes the latest changes and may contain bugs or unfinished features."}
                  </p>
                  {!stable && (
                    <p className="mt-2 text-muted">
                      No stable release available.
                    </p>
                  )}
                </div>
              </div>
            )}
          </div>
        )}
        {releaseError ? (
          <p className="mt-6 text-muted">
            Could not load downloads.{" "}
            <a
              className="link"
              href="https://github.com/neoOMSI/neoOMSI/releases"
            >
              View releases on GitHub
            </a>
            .
          </p>
        ) : releaseList === undefined ? (
          <p className="mt-6 text-muted">Loading available builds…</p>
        ) : !release ? (
          <p className="mt-6 text-muted">No builds have been published yet.</p>
        ) : null}
        {release && (
          <div className="mt-8 flex flex-wrap items-center gap-x-6 gap-y-3">
            {mine && file ? (
              <>
                <a
                  className="btn gap-2"
                  href={file.browser_download_url}
                  download
                >
                  <PlatformIcon build={mine.key} size={20} />
                  Download
                  {release.prerelease
                    ? ` ${selected!.name.toLowerCase()}`
                    : ""}{" "}
                  for {mine.name}
                </a>
                <button
                  type="button"
                  onClick={jump}
                  className="link text-[16px]"
                >
                  Other systems
                </button>
              </>
            ) : (
              <button type="button" onClick={jump} className="btn gap-2">
                <Icon name="download" size={20} />
                Choose a build
              </button>
            )}
          </div>
        )}
        <p className="mt-8 flex max-w-[34em] gap-3 text-[16px] text-muted">
          <Icon
            name="info"
            size={20}
            style={{ marginTop: 3, color: "var(--color-accent)" }}
          />
          <span>
            Needs an installed copy of OMSI&nbsp;2. On the first start, point
            the launcher to its folder, the one with <code>maps</code> and{" "}
            <code>Vehicles</code>. Nothing in it is changed.
          </span>
        </p>
      </PageHead>

      {release && (
        <section className="bleed">
          <h2 id="dl-all" className="section-title scroll-mt-24">
            Download for your system
          </h2>
          <p className="mt-2 text-muted">
            Choose your operating system and processor. All downloads below are
            from the selected {selected!.name.toLowerCase()} build.
          </p>
          <div className="mt-8 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            <Tiles builds={DOWNLOAD_PLATFORMS} release={release} mine={mine} />
          </div>
        </section>
      )}

      <div className="wrap pb-20 sm:pb-24">
        <div className="mt-24 grid grid-cols-[minmax(0,1fr)] gap-x-16 gap-y-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
          <div>
            <h2 className="section-title">Installing</h2>
            <p className="mt-3 max-w-[24em] text-[16px] text-muted">
              Put mods in the folder next to the game, or add them on the
              launcher's <b className="text-ink">Mods</b> page. The original
              OMSI&nbsp;2 folder is never changed.
            </p>
          </div>
          <div>
            <div role="tablist" className="tabs">
              {TABS.map((t) => (
                <button
                  key={t}
                  role="tab"
                  type="button"
                  aria-selected={t === tab}
                  onClick={() => setTab(t)}
                  className="tab"
                >
                  <PlatformIcon
                    build={PLATFORMS.find((p) => p.family === t)!.key}
                    size={18}
                  />
                  {t}
                </button>
              ))}
            </div>
            <p
              role="tabpanel"
              className="mt-6 max-w-[40em] text-[17px] leading-relaxed"
            >
              {INSTALL[tab]}
            </p>
          </div>
        </div>

        {release && (
          <div className="mt-24 grid grid-cols-[minmax(0,1fr)] gap-x-16 gap-y-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
            <div>
              <h2 className="section-title">Dedicated server</h2>
              <p className="mt-3 max-w-[24em] text-[16px] text-muted">
                Only for hosting a multiplayer session without playing on that
                machine. See{" "}
                <a className="link" href={url(docPath("SERVER"))}>
                  Dedicated server
                </a>
                .
              </p>
            </div>
            <div>
              <p className="mb-4 text-muted">
                Same version as the game download: {version(release)}.
              </p>
              <div className="grid gap-3 sm:grid-cols-2">
                <Tiles builds={SERVERS} release={release} />
              </div>
            </div>
          </div>
        )}

        <p className="mt-24 flex items-center gap-2 text-muted">
          <Icon name="history" size={20} />
          <span>
            Older versions are on the{" "}
            <a className="link" href={url("/releases/")}>
              Releases
            </a>{" "}
            page. See{" "}
            <a className="link" href={url(docPath("RELEASING"))}>
              Releasing &amp; versioning
            </a>
            .
          </span>
        </p>
      </div>
    </>
  );
}
