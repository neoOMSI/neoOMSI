import { useEffect, useRef, type ReactNode } from "react";
import wordmark from "../../../assets/logos/wordmark-gradient-dark.svg";
import wordmarkLight from "../../../assets/logos/wordmark-gradient-light.svg";
import { DISCORD, visitorBuild } from "../content/data";
import { DEMOS } from "../components/demos";
import { HOME_FAQ } from "../content/faq";
import { date, latestRelease, version } from "../lib/github";
import { useAsync } from "../lib/hooks";
import { docPath, url } from "../lib/routes";
import { Icon, PlatformIcon } from "../components/icons";
import { mountRoad } from "../gl/road";
import { mountLine } from "../gl/line";
import { useTheme } from "../lib/theme";
import { Answers } from "./Faq";

const FEATURES = [
  [
    "sync_alt",
    "Built for OMSI 2 content",
    "Our goal is to run existing maps, buses and scripts without conversion. Some content is not compatible yet.",
  ],
  [
    "speed",
    "Modern 64-bit engine",
    "Designed for large maps and detailed buses without OMSI 2's 32-bit process memory limit.",
  ],
  [
    "monitor",
    "Modern graphics",
    "Hardware-accelerated graphics using DirectX 12, Metal and Vulkan.",
  ],
  [
    "install_desktop",
    "Cross-platform",
    "Try development builds on Windows, macOS and Linux.",
  ],
  [
    "departure_board",
    "Integrated launcher",
    "Choose a map, bus and timetable, with vehicle previews and controller settings.",
  ],
  [
    "dns",
    "Dedicated server",
    "A headless server for hosting multiplayer sessions on Windows and Linux.",
  ],
  [
    "extension",
    "Separate mod folder",
    "Install add-ons separately without changing your original OMSI 2 files.",
  ],
  [
    "public",
    "Free and open source",
    "The engine is available under GPL-3.0-or-later. Original OMSI 2 game content is not included.",
  ],
  [
    "check_circle",
    "Reviewed and tested",
    "Changes are reviewed before they are merged, and compatibility tests are added as systems are verified.",
  ],
];

const STEPS: [string, ReactNode][] = [
  [
    "Install OMSI 2",
    "neoOMSI contains no game content. It plays on the maps, vehicles and files of an installed copy of OMSI 2 and does not start without one.",
  ],
  [
    "Download neoOMSI",
    <>
      Get the build for your system from the{" "}
      <a className="link" href={url("/download/")}>
        download page
      </a>{" "}
      and unpack it into a folder of your own.
    </>,
  ],
  [
    "Choose your OMSI 2 folder",
    <>
      On the first start, point the launcher to the folder with{" "}
      <code>maps</code> and <code>Vehicles</code>. Nothing in it is changed.
    </>,
  ],
  ["Drive", "Pick a map, a bus and a duty in the launcher."],
];

const STOPS = [
  [
    "Download",
    "Windows, macOS and Linux",
    url("/download/"),
  ],
  ["User guide", "Setup, controls and options", url(docPath("USER_GUIDE"))],
  ["Releases", "Every version and what changed", url("/releases/")],
  ["Discord", "Talk to players and developers", DISCORD],
];

const SHEEN =
  "linear-gradient(115deg, rgb(255 255 255 / .22) 0%, rgb(255 255 255 / 0) 38%)";

const BACKDROPS = [
  `${SHEEN}, linear-gradient(160deg, #ff9a4d 0%, #fd6b00 40%, #c24f08 100%)`,
  `${SHEEN}, linear-gradient(160deg, #4f86e0 0%, #2456b0 45%, #12306e 100%)`,
  `${SHEEN}, linear-gradient(165deg, #c4a8f0 0%, #8a6ad8 45%, #46349a 100%)`,
  `${SHEEN}, linear-gradient(160deg, #6fd8c0 0%, #1f9c86 45%, #0c4f45 100%)`,
];

function RoadHero({ children }: { children: ReactNode }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const { theme } = useTheme();
  useEffect(() => mountRoad(canvas.current!, theme), [theme]);
  return (
    <section className="relative h-svh min-h-[38rem] overflow-hidden">
      <canvas
        ref={canvas}
        className="absolute top-0 left-0"
        aria-hidden="true"
      />
      <div className="wrap relative pt-28 sm:pt-44">{children}</div>
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-[45%] bg-linear-to-b from-transparent to-page" />
    </section>
  );
}

// The mockups keep the app's real pixel sizes; on narrow stages they shrink, but still run off the edge.
function Stage({
  backdrop,
  children,
}: {
  backdrop: string;
  children: ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = box.current!;
    const demo = el.firstElementChild as HTMLElement;
    const observer = new ResizeObserver(() => {
      demo.style.zoom = "";
      demo.style.zoom = String(
        Math.min(1, (el.clientWidth * 1.12) / demo.offsetWidth),
      );
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <div className="stage" style={{ background: backdrop }}>
      <div ref={box} className="stage-fit">
        {children}
      </div>
    </div>
  );
}

function Line() {
  const canvas = useRef<HTMLCanvasElement>(null);
  const stops = useRef<HTMLElement[]>([]);
  const { theme } = useTheme();
  useEffect(() => mountLine(canvas.current!, stops.current, theme), [theme]);
  return (
    <div className="relative mt-12 sm:mt-14">
      <canvas
        ref={canvas}
        className="pointer-events-none absolute top-0 left-0"
        aria-hidden="true"
      />
      <ol className="wrap relative grid gap-y-8 py-2 sm:grid-cols-4 sm:gap-x-8 sm:py-0">
        {STOPS.map(([title, text, href], i) => (
          <li key={title}>
            <a href={href} className="group flex gap-5 sm:block">
              <span
                ref={(el) => void (stops.current[i] = el!)}
                className="block size-5 shrink-0 sm:mt-4 sm:mb-6"
              />
              <span className="block">
                <span className="block text-[1.15rem] font-semibold text-heading group-hover:text-accent">
                  {title}
                </span>
                <span className="mt-0.5 block text-[16px] text-muted">
                  {text}
                </span>
              </span>
            </a>
          </li>
        ))}
      </ol>
    </div>
  );
}

export function Home() {
  const build = visitorBuild();
  const { data: release } = useAsync(latestRelease, []);

  return (
    <>
      <RoadHero>
        <h1 className="w-[clamp(16rem,40vw,32rem)]">
          <img className="logo-dark w-full" src={wordmark} alt="" />
          <img className="logo-light w-full" src={wordmarkLight} alt="" />
          <span className="sr-only">neoOMSI: OMSI 2 rebuilt for modern systems</span>
        </h1>
        <p className="mt-7 max-w-[32em] text-[20px] leading-relaxed text-muted">
          <span className="text-heading">OMSI 2, rebuilt for modern systems.</span>{" "}
          neoOMSI is a free, open-source project aiming to run your existing
          maps, buses and scripts on a new 64-bit engine.
        </p>
        <div className="mt-7 flex flex-wrap gap-3">
          <a className="btn gap-2" href={url("/download/")}>
            <PlatformIcon build={build?.key} size={20} />
            {build ? `Download for ${build.name}` : "Download"}
          </a>
          <a className="btn-quiet gap-2" href={url(docPath("USER_GUIDE"))}>
            <Icon name="description" size={20} />
            Read the user guide
          </a>
        </div>
        <p className="mt-5 text-[15px] text-muted">
          {release && `Build ${version(release)} · ${date(release.published_at)}. `}
          {release && !release.prerelease
            ? "Compatibility is still being improved."
            : "Early development. Expect bugs and missing features."}
        </p>
      </RoadHero>

      <section className="relative -mt-[18svh] sm:-mt-[34svh]">
        <div className="bleed grid gap-x-16 gap-y-10 py-16 sm:py-20 lg:grid-cols-[minmax(0,17rem)_minmax(0,1fr)]">
          <div>
            <h2 className="text-[2rem] leading-tight">What we're building</h2>
            <p className="mt-3 max-w-[22em] text-muted">
              A work-in-progress replacement for OMSI 2, with compatibility improved
              step by step against the original game.
            </p>
          </div>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-[repeat(3,minmax(0,15vw))]">
            {FEATURES.map(([symbol, title, text]) => (
              <div key={title} className="card flex flex-col gap-3 p-5">
                <span className="text-accent">
                  <Icon name={symbol} size={28} />
                </span>
                <div>
                  <h3 className="text-[1.1rem]">{title}</h3>
                  <p className="mt-1 text-[16px] text-muted">{text}</p>
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section>
        <div className="wrap grid gap-x-16 gap-y-10 py-16 sm:py-20 lg:grid-cols-[1fr_2fr]">
          <h2 className="text-[2rem] leading-tight">Getting started</h2>
          <ol className="route max-w-[38rem]">
            {STEPS.map(([title, text]) => (
              <li key={title}>
                <h3 className="text-[1.2rem]">{title}</h3>
                <p className="mt-1 text-muted">{text}</p>
              </li>
            ))}
          </ol>
        </div>
      </section>

      <section>
        <div className="wrap py-16 sm:py-20">
          <h2 className="max-w-[22em] text-[2rem] leading-tight">
            Inside neoOMSI.{" "}
            <span className="text-muted">
              From picking a bus in the launcher to driving the route.
            </span>
          </h2>
          <div className="mt-12 grid gap-4 lg:grid-cols-2">
            {DEMOS.map(({ title, text, Demo }, i) => {
              const wide = i === 0 || i === DEMOS.length - 1;
              const flip = i === DEMOS.length - 1;
              return (
                <article
                  key={title}
                  className={`showcase${wide ? " wide lg:col-span-2" : ""}${flip ? " flip" : ""}`}
                >
                  <p className="showcase-text">
                    <span className="font-semibold text-heading">{title}.</span>{" "}
                    {text}
                  </p>
                  <Stage backdrop={BACKDROPS[i]}>
                    <Demo />
                  </Stage>
                </article>
              );
            })}
          </div>
        </div>
      </section>

      <section>
        <div className="wrap grid gap-x-16 gap-y-10 py-16 sm:py-20 lg:grid-cols-[1fr_2fr]">
          <div>
            <h2 className="text-[2rem] leading-tight">Questions</h2>
            <p className="mt-3 text-muted">
              More in the{" "}
              <a className="link" href={url("/faq/")}>
                FAQ
              </a>
              .
            </p>
          </div>
          <Answers list={HOME_FAQ} level={3} />
        </div>
      </section>

      <section className="pt-16 pb-24 sm:pt-20 sm:pb-32">
        <div className="wrap">
          <h2 className="max-w-[16em] text-[clamp(1.8rem,3.5vw,2.5rem)] leading-tight">
            Your maps, your buses, your mods.
          </h2>
          <p className="mt-3 text-muted">
            neoOMSI reads them from your OMSI&nbsp;2 installation without
            modifying it.
          </p>
        </div>
        <Line />
      </section>
    </>
  );
}
