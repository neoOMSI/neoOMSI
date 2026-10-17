import type { ReactNode } from "react";
import { PageHead } from "../components/ui";
import { OPENOMSI, REPO } from "../content/data";
import { OPENOMSI_FAQ } from "../content/faq";
import { Icon } from "../components/icons";
import { docPath, url } from "../lib/routes";
import { Answers } from "./Faq";

const ROWS: [string, ReactNode, ReactNode][] = [
  ["Original OMSI 2 files", "Required; not included", "Required; not included"],
  ["Codebase", "Based on an earlier version of openOMSI", "Original openOMSI project"],
  ["Maintained by", "The neoOMSI team", "The openOMSI team"],
  ["Downloads", "Windows, macOS and Linux development builds", "See the official openOMSI releases"],
];

const FOCUS: [string, string, ReactNode][] = [
  [
    "check_circle",
    "Code review",
    "Changes to main require two approving reviews, passing required checks, and resolved review threads.",
  ],
  [
    "sync_alt",
    "Checking against OMSI 2",
    <>
      Compatibility-sensitive behavior is checked against OMSI&nbsp;2.2.032,
      with focused regression tests added as subsystems are verified. See{" "}
      <a className="link" href={url(docPath("COMPATIBILITY"))}>
        Compatibility
      </a>
      .
    </>,
  ],
  [
    "public",
    "Uses your OMSI 2 installation",
    "neoOMSI does not distribute original OMSI 2 binaries or game assets. It reads content from an installation you provide.",
  ],
  [
    "autorenew",
    "Development releases",
    <>
      Development snapshots are published as Nightly builds. Release
      Candidates and Stable releases are planned for stabilization milestones. See{" "}
      <a className="link" href={url("/releases/")}>
        Releases
      </a>
      .
    </>,
  ],
];

const STEPS: ReactNode[] = [
  <>
    Download the neoOMSI build for your system from the{" "}
    <a className="link" href={url("/download/")}>
      download page
    </a>{" "}
    and extract it into a dedicated folder.
  </>,
  "Launch neoOMSI and select the same OMSI 2 installation folder you use with openOMSI.",
  <>
    Place any custom add-ons into the <code>Mods</code> folder next to neoOMSI, or
    manage them on the launcher's <b className="text-ink">Mods</b> page.
  </>,
  "Select a map, vehicle, and timetable duty in the launcher. Your original OMSI 2 files remain untouched, allowing both engines to be tested independently.",
];

export function OpenOmsi() {
  return (
    <>
      <PageHead>
        <p className="mb-4 text-[15px] font-semibold text-accent">Comparison</p>
        <h1 className="display">neoOMSI and openOMSI</h1>
        <p className="mt-6 max-w-[36em] text-[19px] text-muted">
          neoOMSI is based on an earlier version of openOMSI, but the projects
          are now developed by separate teams. Here's how they relate and
          how to try neoOMSI alongside openOMSI.
        </p>
        <div className="mt-8 flex flex-wrap gap-3">
          <a className="btn gap-2" href={url("/download/")}>
            <Icon name="download" size={20} />
            Download neoOMSI
          </a>
          <a className="btn-quiet gap-2" href={url("/faq/")}>
            <Icon name="info" size={20} />
            Read the FAQ
          </a>
        </div>
      </PageHead>

      <div className="wrap space-y-24 pb-20 sm:pb-24">
        <section>
          <h2 className="section-title">At a glance</h2>
          <div className="mt-8 overflow-x-auto rounded-xl border border-line">
            <table className="w-full border-collapse text-left text-[16px]">
              <thead className="bg-sunken text-[14px] text-muted">
                <tr>
                  <th scope="col" className="px-5 py-3 font-semibold">
                    <span className="sr-only">Topic</span>
                  </th>
                  <th
                    scope="col"
                    className="px-5 py-3 font-semibold text-heading"
                  >
                    neoOMSI
                  </th>
                  <th scope="col" className="px-5 py-3 font-semibold">
                    openOMSI
                  </th>
                </tr>
              </thead>
              <tbody>
                {ROWS.map(([label, neo, open]) => (
                  <tr key={label} className="border-t border-line align-top">
                    <th
                      scope="row"
                      className="px-5 py-3 font-medium text-muted"
                    >
                      {label}
                    </th>
                    <td className="px-5 py-3 text-heading">{neo}</td>
                    <td className="px-5 py-3">{open}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="mt-4 text-[15px] text-muted">
            For current openOMSI features and downloads, refer to the{" "}
            <a className="link" href={OPENOMSI.repo}>
              openOMSI repository
            </a>.
          </p>
        </section>

        <section className="grid gap-x-16 gap-y-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
          <h2 className="section-title">How the projects relate</h2>
          <div className="max-w-[40em] space-y-4">
            <p>
              neoOMSI originated from an earlier MIT-licensed snapshot of openOMSI
              by usonskyyyy. Both codebases share common foundations, and upstream
              copyright notices and license terms are preserved in the{" "}
              <a
                className="link"
                href={`https://github.com/${REPO}/blob/main/NOTICE`}
              >
                NOTICE
              </a>{" "}
              file and repository documentation.
            </p>
            <p>
              neoOMSI was created to follow a different development and review
              process, with a focus on OMSI 2 compatibility and long-term code
              quality. Both projects now evolve independently, so improvements
              in one do not automatically appear in the other.
            </p>
            <p>
              Our changes require reviews and automated checks before they reach
              the main branch. Compatibility with OMSI 2 is tested piece by piece,
              and is not yet complete.
            </p>
          </div>
        </section>

        <section>
          <h2 className="section-title">How neoOMSI is developed</h2>
          <div className="mt-8 grid gap-3 sm:grid-cols-2">
            {FOCUS.map(([symbol, title, text]) => (
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
        </section>

        <section className="grid gap-x-16 gap-y-10 lg:grid-cols-[1fr_2fr]">
          <h2 className="section-title">Switching from openOMSI</h2>
          <ol className="route max-w-[38rem]">
            {STEPS.map((step, i) => (
              <li key={i}>
                <p className="text-muted">{step}</p>
              </li>
            ))}
          </ol>
        </section>

        <section>
          <h2 className="section-title">
            Questions about neoOMSI and openOMSI
          </h2>
          <div className="mt-6">
            <Answers list={OPENOMSI_FAQ} level={3} />
          </div>
        </section>
      </div>
    </>
  );
}
