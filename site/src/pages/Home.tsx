import { useEffect, useRef, type ReactNode } from "react";
import wordmark from "../../../assets/logos/wordmark-gradient-dark.svg?trim";
import wordmarkLight from "../../../assets/logos/wordmark-gradient-light.svg?trim";
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
    "Behavioral parity",
    "Targets OMSI 2.2.032 behavior so existing maps, buses, splines and scripts can run without manual conversion.",
  ],
  [
    "speed",
    "64-bit engine",
    "Multithreaded tile streaming, background texture decompression and no arbitrary memory ceilings.",
  ],
  [
    "monitor",
    "Modern graphics",
    "Native hardware acceleration through wgpu: DirectX 12 on Windows, Metal on macOS, Vulkan on Linux.",
  ],
  [
    "install_desktop",
    "Cross-platform",
    "Native support for Windows, macOS, Linux and Android from the same Rust codebase.",
  ],
  [
    "departure_board",
    "Integrated launcher",
    "Scenario setup, duty and timetable selection, vehicle previews and controller configuration.",
  ],
  [
    "dns",
    "Dedicated server",
    "Host multiplayer sessions headless. Maps and timetables are simulated without a GPU.",
  ],
  [
    "extension",
    "Non-destructive modding",
    "Add-ons live in a separate overlay, so trying and removing them never touches your OMSI 2 files.",
  ],
  [
    "public",
    "Clean-room and open source",
    "No proprietary OMSI binaries or game assets are included. GPL-3.0-or-later, developed in the open.",
  ],
  [
    "check_circle",
    "Regression tested",
    "Focused regression tests protect verified compatibility behavior from regressions.",
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
    "Windows, macOS, Linux; Android builds locally",
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
          <span className="sr-only">neoOMSI: OMSI 2 recreated in Rust</span>
        </h1>
        <p className="mt-7 max-w-[32em] text-[20px] leading-relaxed text-muted">
          <span className="text-heading">OMSI 2, reimplemented in Rust.</span>{" "}
          Built for existing OMSI 2 maps, buses and mods, with compatibility
          verified against OMSI 2.2.032 on a modern 64-bit engine.
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
          {release &&
            `Version ${version(release)}, released ${date(release.published_at)}. `}
          Early release, expect bugs.
        </p>
      </RoadHero>

      <section className="relative -mt-[18svh] sm:-mt-[34svh]">
        <div className="bleed grid gap-x-16 gap-y-10 py-16 sm:py-20 lg:grid-cols-[minmax(0,17rem)_minmax(0,1fr)]">
          <div>
            <h2 className="text-[2rem] leading-tight">What's different</h2>
            <p className="mt-3 max-w-[22em] text-muted">
              An open-source reimplementation that targets the observable
              behavior of OMSI&nbsp;2.2.032.
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
