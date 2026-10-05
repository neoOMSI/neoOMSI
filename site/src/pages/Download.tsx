import { useState, type ReactNode } from "react";
import { PageHead } from "../components/ui";
import { PLATFORMS, SERVERS, visitorBuild, type Build } from "../content/data";
import {
  date,
  latestRelease,
  megabytes,
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
  Android: (
    <>
      Android packages are currently built locally rather than published by the
      automated release workflow. See{" "}
      <a className="link" href={url(docPath("BUILDING"))}>
        Building
      </a>
      . After building, install the <code>.apk</code>, allow access to all
      files, and copy the whole OMSI&nbsp;2 folder to{" "}
      <code>neoOMSI/OMSI 2</code> on the device.
    </>
  ),
};

const TABS = Object.keys(INSTALL);

const asset = (release: Release | null | undefined, build: Build) => {
  const file =
    release && `neoOMSI-${version(release)}-${build.key}.${build.ext || "zip"}`;
  return release?.assets.find((a) => a.name === file);
};

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
  const mine = visitorBuild();
  const [tab, setTab] = useState(() =>
    TABS.includes(mine?.family ?? "") ? mine!.family : TABS[0],
  );
  const { data: release } = useAsync(latestRelease, []);
  const file = mine && asset(release, mine);

  return (
    <>
      <PageHead>
        <h1 className="display">Download</h1>
        <p className="mt-6 max-w-[38em] text-[19px] text-muted">
          {release === undefined
            ? "Looking up the latest release…"
            : release
              ? `Version ${version(release)}, released ${date(release.published_at)}. An early release: expect bugs and changes between versions.`
              : "No release has been published yet. Builds appear here as soon as the first one is out."}
        </p>
        <div className="mt-8 flex flex-wrap items-center gap-x-6 gap-y-3">
          {mine && file ? (
            <>
              <a
                className="btn gap-2"
                href={file.browser_download_url}
                download
              >
                <PlatformIcon build={mine.key} size={20} />
                Download for {mine.name}
              </a>
              <button type="button" onClick={jump} className="link text-[16px]">
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

      <section className="bleed">
        <h2 id="dl-all" className="section-title scroll-mt-24">
          All builds
        </h2>
        <p className="mt-2 text-muted">
          Every build plays the same maps and buses. Pick the one that matches
          your device.
        </p>
        <div className="mt-8 grid gap-3 md:grid-cols-2">
          <Tiles builds={PLATFORMS} release={release} mine={mine} />
        </div>
      </section>

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
          <div className="grid gap-3 sm:grid-cols-2">
            <Tiles builds={SERVERS} release={release} />
          </div>
        </div>

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
