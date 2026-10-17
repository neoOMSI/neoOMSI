import type { CSSProperties, ReactNode } from "react";
import { Icon } from "./icons";

const EM = 2048 / 2400;
const ACCENT = "#e8a030";
const TEXT = "var(--a-text)";
const SOFT = "var(--a-soft)";
const DIM = "var(--a-dim)";
const FAINT = "var(--a-faint)";

const font = (size: number, weight = 400, color = TEXT): CSSProperties => ({
  fontSize: +(size * EM).toFixed(2),
  fontWeight: weight,
  color,
});

const line = (
  h: number,
  size: number,
  weight = 400,
  color = TEXT,
): CSSProperties => ({
  ...font(size, weight, color),
  lineHeight: `${h}px`,
  height: h,
});

const baseline = (size: number, y: number): CSSProperties => {
  const em = size * EM;
  return {
    lineHeight: `${+(2 * (y - 0.928 * em) + 1.172 * em).toFixed(2)}px`,
  };
};

type Box = { children?: ReactNode; style?: CSSProperties };

const Heading = ({ children }: Box) => (
  <div
    className="truncate uppercase"
    style={{ height: 26, ...font(11, 700, DIM), ...baseline(11, 14) }}
  >
    {children}
  </div>
);

const Label = ({ children, h = 20, style }: Box & { h?: number }) => (
  <div className="truncate" style={{ ...line(h, 13, 500, DIM), ...style }}>
    {children}
  </div>
);

const Field = ({ h, children, style }: Box & { h: number }) => (
  <div className="a-field" style={{ height: h, ...style }}>
    {children}
  </div>
);

const At = ({ x, y, children }: Box & { x: number; y?: number }) => (
  <span className="a-at" style={{ left: x, top: y }}>
    {children}
  </span>
);

const AtRight = ({ x, children }: Box & { x: number }) => (
  <span className="a-at" style={{ right: x, transform: "translate(50%,-50%)" }}>
    {children}
  </span>
);

const Text = ({
  children,
  style,
  className = "",
}: Box & { className?: string }) => (
  <span className={`a-text ${className}`} style={style}>
    {children}
  </span>
);

const Input = ({
  placeholder,
  icon,
  h = 36,
}: {
  placeholder: string;
  icon: string;
  h?: number;
}) => (
  <Field h={h}>
    <At x={20}>
      <Icon name={icon} size={18} color={DIM} />
    </At>
    <Text
      style={{
        left: 36,
        ...font(13, 450, FAINT),
        lineHeight: `${h - 2}px`,
      }}
    >
      {placeholder}
    </Text>
  </Field>
);

const Select = ({
  value,
  h = 36,
  style,
}: {
  value: string;
  h?: number;
  style?: CSSProperties;
}) => (
  <Field h={h} style={style}>
    <Text
      style={{
        left: 12,
        right: 28,
        ...font(13),
        lineHeight: `${h - 2}px`,
      }}
    >
      {value}
    </Text>
    <AtRight x={18}>
      <Icon name="expand_more" size={20} color={DIM} />
    </AtRight>
  </Field>
);

const Toggle = ({ text, on, h }: { text: string; on: boolean; h: number }) => (
  <div className="relative flex items-center" style={{ height: h }}>
    <span
      className="truncate"
      style={{ width: "calc(100% - 44px)", ...font(13, 400, SOFT) }}
    >
      {text}
    </span>
    <i className={`a-toggle${on ? " on" : ""}`} />
  </div>
);

const TABS = ["Bus", "Route", "Time & weather", "Roadbook"];

const Panel = ({
  active,
  children,
}: {
  active: number;
  children: ReactNode;
}) => (
  <div className="app">
    <div className="a-tabs">
      {TABS.map((t, i) => (
        <span
          key={t}
          className={i === active ? "on" : ""}
          style={line(
            40,
            13,
            i === active ? 500 : 400,
            i === active ? TEXT : DIM,
          )}
        >
          {t}
        </span>
      ))}
    </div>
    <div className="a-body">{children}</div>
  </div>
);

function BusRow({
  title,
  sub,
  right,
  star,
  chosen = false,
}: {
  title: string;
  sub: string;
  right: ReactNode;
  star?: ReactNode;
  chosen?: boolean;
}) {
  return (
    <div className={`a-row${chosen ? " sel" : ""}`} style={{ height: 54 }}>
      <At x={20} y={23}>
        <Icon name="directions_bus" size={20} color={chosen ? ACCENT : DIM} />
      </At>
      <Text style={{ left: 42, top: 7, ...line(20, 13, 500) }}>{title}</Text>
      <Text style={{ left: 42, top: 29, ...line(17, 11.5, 400, DIM) }}>
        {sub}
      </Text>
      {star && <AtRight x={49}>{star}</AtRight>}
      <AtRight x={18}>{right}</AtRight>
    </div>
  );
}

function Bus() {
  return (
    <Panel active={0}>
      <Heading>Choose a bus</Heading>
      <div style={{ marginTop: 6 }}>
        <Input placeholder="Search buses…" icon="search" />
      </div>
      <div className="relative" style={{ height: 22, marginTop: 4 }}>
        <span
          className="absolute left-0"
          style={{ top: 2, ...line(20, 11.5, 400, DIM) }}
        >
          4 manufacturers
        </span>
        <div className="absolute top-0 right-0" style={{ width: 190 }}>
          <Toggle text="Favourites only" on={false} h={22} />
        </div>
      </div>
      <div className="a-list" style={{ marginTop: 6 }}>
        <BusRow
          title="MAN"
          sub="NL202 · 4 models"
          right={<Icon name="expand_less" size={18} color={ACCENT} />}
          star={
            <Icon
              name="star"
              size={14}
              color={ACCENT}
              style={{ opacity: 0.8 }}
            />
          }
          chosen
        />
        <div style={{ marginLeft: 32, marginTop: 4 }}>
          <div style={line(22, 13, 500, DIM)}>Type / variant</div>
          <div className="flex" style={{ marginTop: 4, gap: 6 }}>
            <Select value="NL202 (2-door)" style={{ flex: 1 }} />
            <span
              className="grid place-items-center"
              style={{ width: 30, height: 36 }}
            >
              <Icon name="star" size={18} color={ACCENT} />
            </span>
          </div>
        </div>
        <div style={{ height: 10 }} />
        <BusRow
          title="Mercedes-Benz"
          sub="O 530"
          right={<Icon name="chevron_right" size={18} color={DIM} />}
          star={<Icon name="star" size={16} color="rgb(255 255 255 / .16)" />}
        />
        <BusRow
          title="Neoplan"
          sub="3 models"
          right={<Icon name="expand_more" size={18} color={DIM} />}
        />
        <BusRow
          title="Volvo"
          sub="7700"
          right={<Icon name="chevron_right" size={18} color={DIM} />}
          star={<Icon name="star" size={16} color={ACCENT} />}
        />
        <i className="a-scroll" style={{ top: 8, height: 120 }} />
      </div>
      <div className="flex justify-between" style={{ marginTop: 16 }}>
        <Label h={22}>Livery</Label>
        <span style={line(22, 11.5, 400, DIM)}>1 / 6</span>
      </div>
      <div className="flex" style={{ marginTop: 6, gap: 8 }}>
        <Select value="Default paint" style={{ flex: 1 }} />
        <span className="a-button">
          <Icon name="chevron_left" size={17} />
        </span>
        <span className="a-button">
          <Icon name="chevron_right" size={17} />
        </span>
      </div>
      <div className="relative" style={{ height: 32, marginTop: 14 }}>
        <At x={12}>
          <Icon name="expand_more" size={18} color={DIM} />
        </At>
        <Text style={{ left: 30, ...line(32, 12.5, 500, DIM) }}>
          Vehicle settings &amp; details
        </Text>
      </div>
    </Panel>
  );
}

const LINES = [
  ["24", "Hauptbahnhof · Rathaus", 6, true],
  ["36", "Waldweg · Markt", 4, false],
  ["N7", "Bahnhof · Siedlung", 2, false],
] as const;

const TOURS = [
  ["Tour 2", "07:12 - 08:40", "6 trips · Mon-Fri", true],
  ["Tour 3", "07:42 - 09:10", "6 trips · Mon-Fri", false],
] as const;

function Route() {
  return (
    <Panel active={1}>
      <div className="flex">
        <Label h={36} style={{ width: 110, flex: "none" }}>
          Map
        </Label>
        <Select value="Grundorf" style={{ flex: 1 }} />
      </div>
      <div style={{ marginTop: 8 }}>
        <Toggle text="Free drive (no timetable duty)" on={false} h={36} />
      </div>
      <div className="flex" style={{ marginTop: 10 }}>
        <Label h={36} style={{ width: 110, flex: "none" }}>
          Start at
        </Label>
        <Select
          value="Automatic (nearest to the first stop)"
          style={{ flex: 1 }}
        />
      </div>
      <div className="grid grid-cols-2" style={{ marginTop: 10, gap: 12 }}>
        <div>
          <Heading>Line</Heading>
          <div style={{ marginTop: 4 }}>
            <Input placeholder="Filter…" icon="search" h={34} />
          </div>
          <div
            className="flex flex-col"
            style={{
              marginTop: 8,
              gap: 4,
              width: "calc(100% - 4px)",
            }}
          >
            {LINES.map(([name, termini, count, chosen]) => (
              <div
                key={name}
                className={`a-row${chosen ? " sel" : ""}`}
                style={{ height: 44 }}
              >
                <span className="a-badge" style={{ left: 8, top: 8 }}>
                  {name}
                </span>
                <span
                  className="absolute flex items-center"
                  style={{
                    right: 4,
                    top: 8,
                    height: 22,
                    gap: 4,
                  }}
                >
                  <Icon name="event" size={13} color={FAINT} />
                  <span style={font(11.5, 700, DIM)}>{count}</span>
                </span>
                <Text
                  style={{
                    left: 8,
                    right: 8,
                    top: 28,
                    ...line(16, 11, 400, FAINT),
                  }}
                >
                  {termini}
                </Text>
              </div>
            ))}
          </div>
        </div>
        <div>
          <Heading>Tour</Heading>
          <div className="flex flex-col" style={{ marginTop: 4, gap: 4 }}>
            {TOURS.map(([name, time, days, chosen]) => (
              <div
                key={name}
                className={`a-row${chosen ? " sel" : ""}`}
                style={{ height: 80 }}
              >
                <Text
                  style={{
                    left: 10,
                    top: 5,
                    ...line(18, 13.5, 700),
                  }}
                >
                  {name}
                </Text>
                <Text
                  className="text-right"
                  style={{
                    right: 10,
                    top: 6,
                    width: 100,
                    ...line(18, 12, 500, ACCENT),
                  }}
                >
                  {time}
                </Text>
                <Text
                  style={{
                    left: 10,
                    right: 10,
                    top: 25,
                    ...line(16, 11.5, 500),
                  }}
                >
                  Hauptbahnhof → Rathaus
                </Text>
                <Text
                  style={{
                    left: 10,
                    right: 10,
                    top: 43,
                    ...line(16, 12, 500),
                  }}
                >
                  Trip duration: 1 hour 28 minutes
                </Text>
                <Text
                  style={{
                    left: 10,
                    right: 10,
                    top: 61,
                    ...line(16, 11, 400, DIM),
                  }}
                >
                  {days}
                </Text>
              </div>
            ))}
          </div>
        </div>
      </div>
    </Panel>
  );
}

const Half = ({ value }: { value: string }) => (
  <div className="relative flex-1">
    <span
      className="absolute text-center"
      style={{ left: 8, right: 22, ...line(42, 17, 500) }}
    >
      {value}
    </span>
    <span
      className="absolute flex flex-col items-center"
      style={{ right: 4, top: 3, bottom: 3, width: 20 }}
    >
      <span className="grid flex-1 place-items-center">
        <Icon name="expand_less" size={16} color={FAINT} />
      </span>
      <span className="grid flex-1 place-items-center">
        <Icon name="expand_more" size={16} color={FAINT} />
      </span>
    </span>
  </div>
);

const WEATHERS = [
  ["wb_sunny", "Map default", "Whatever the map starts with", false],
  [
    "tune",
    "Custom weather",
    "Set visibility, wind, clouds, rain, temperature and road state",
    false,
  ],
  ["public", "Current weather", "METAR of EDDB (fetched at the start)", false],
  [
    "autorenew",
    "Weather cycle",
    "Changes every 25-60 minutes, as the month allows",
    true,
  ],
] as const;

const SEASONS = ["By date", "Spring", "Summer", "Autumn", "Winter"];

function Weather() {
  return (
    <Panel active={2}>
      <div className="grid grid-cols-2" style={{ gap: "0 12px" }}>
        <Label>Time</Label>
        <Label>Date</Label>
        <div style={{ marginTop: 2 }}>
          <Field h={44}>
            <div className="flex size-full">
              <Half value="09" />
              <span
                className="grid place-items-center"
                style={{ width: 16, ...font(17, 500, DIM) }}
              >
                :
              </span>
              <Half value="00" />
            </div>
          </Field>
        </div>
        <div style={{ marginTop: 2 }}>
          <Field h={44}>
            <At x={20}>
              <Icon name="calendar_month" size={17} color={DIM} />
            </At>
            <Text
              style={{
                left: 38,
                ...font(13),
                lineHeight: "42px",
              }}
            >
              30 May 1989
            </Text>
            <AtRight x={18}>
              <Icon name="expand_more" size={20} color={DIM} />
            </AtRight>
          </Field>
        </div>
      </div>
      <div className="a-seg" style={{ marginTop: 10 }}>
        {SEASONS.map((s, i) => (
          <span
            key={s}
            className={i === 0 ? "on" : ""}
            style={font(12.5, 500, i === 0 ? TEXT : DIM)}
          >
            {s}
          </span>
        ))}
      </div>
      <div className="flex items-center" style={{ height: 34, marginTop: 12 }}>
        <span
          className="truncate"
          style={{
            width: 170,
            paddingRight: 8,
            ...font(13, 400, SOFT),
          }}
        >
          Cars around
        </span>
        <div className="a-slider flex-1">
          <i style={{ width: "25%" }} />
        </div>
        <span className="text-right" style={{ width: 58, ...font(12.5, 500) }}>
          30
        </span>
      </div>
      <div className="grid grid-cols-2" style={{ marginTop: 6, gap: "0 12px" }}>
        <Toggle text="Passengers" on h={32} />
        <Toggle text="Timetable buses" on h={32} />
      </div>
      <div style={{ marginTop: 4 }}>
        <Toggle
          text="Put the bus into service on start (Shift+U)"
          on={false}
          h={32}
        />
      </div>
      <div style={{ marginTop: 4 }}>
        <Toggle
          text="Start on foot (place a bus from the game menu)"
          on={false}
          h={32}
        />
      </div>
      <div style={{ marginTop: 12 }}>
        <Heading>Weather</Heading>
      </div>
      <div className="grid grid-cols-2" style={{ marginTop: 4, gap: 8 }}>
        {WEATHERS.map(([symbol, name, meta, chosen]) => (
          <div key={name} className={`a-card${chosen ? " sel" : ""}`}>
            <At x={24}>
              <Icon name={symbol} size={22} color={chosen ? TEXT : DIM} />
            </At>
            <Text
              style={{
                left: 46,
                right: 10,
                top: 10,
                ...line(20, 13, 700),
              }}
            >
              {name}
            </Text>
            <Text
              style={{
                left: 46,
                right: 10,
                top: 33,
                ...line(18, 11, 400, DIM),
              }}
            >
              {meta}
            </Text>
          </div>
        ))}
      </div>
    </Panel>
  );
}

const MAP = {
  w: 360,
  h: 223,
  tilt: (38 * Math.PI) / 180,
  focal: 111.5 / Math.tan((20 * Math.PI) / 180),
  shift: 88.5,
};

function project(x: number, y: number) {
  const dy = y - MAP.shift - MAP.h / 2;
  const k = MAP.focal / (MAP.focal - dy * Math.sin(MAP.tilt));
  return [
    MAP.w / 2 + (x - MAP.w / 2) * k,
    MAP.h / 2 + dy * Math.cos(MAP.tilt) * k,
  ];
}

function Marker({
  x,
  y,
  children,
}: {
  x: number;
  y: number;
  children: ReactNode;
}) {
  const [px, py] = project(x, y);
  return (
    <span
      className="absolute"
      style={{ left: `${px.toFixed(1)}px`, top: `${py.toFixed(1)}px` }}
    >
      {children}
    </span>
  );
}

const Stop = ({
  r,
  fill,
  ring,
  glyph,
  color,
}: {
  r: number;
  fill: string;
  ring: number;
  glyph: number;
  color: string;
}) => (
  <>
    <svg
      className="absolute"
      viewBox="-12 -12 24 24"
      style={{ width: 24, height: 24, left: -12, top: -12 }}
      aria-hidden="true"
    >
      <circle r={ring} fill="rgb(22 22 22 / .92)" />
      <circle r={r} fill={fill} />
    </svg>
    <span
      className="absolute"
      style={{ left: -glyph / 2, top: -glyph / 2, display: "flex" }}
    >
      <Icon name="directions_bus" size={glyph} color={color} />
    </span>
  </>
);

const Car = () => (
  <svg
    className="absolute"
    viewBox="-4 -4 8 8"
    style={{ width: 8, height: 8, left: -4, top: -4 }}
    aria-hidden="true"
  >
    <circle r="3.6" fill="rgb(8 8 8 / .9)" />
    <circle r="2.6" fill="#468cff" />
  </svg>
);

const ROADS = [
  "M-200 140 H560",
  "M180 420 V-300",
  "M60 420 V140 L-40 -60",
  "M300 140 L420 -120",
  "M180 20 H560",
];
const CHEVRONS = [
  ...[330, 290, 250, 210, 170].map(
    (y) => `M176 ${y + 4} l4 -6 l4 6 l-4 -2.4 z`,
  ),
  ...[220, 260, 300, 340, 380, 420].map(
    (x) => `M${x - 4} 136 l6 4 l-6 4 l2.4 -4 z`,
  ),
];
const CARS = [
  [60, 60],
  [-10, 140],
  [180, -60],
  [520, 20],
];
const NEXT = [
  ["07:36", "Marktplatz", "07:36", "#b2b2b2"],
  ["07:39", "Lindenallee", "07:40", "#eb5546"],
  ["07:43", "Hauptbahnhof", "07:44", "#eb5546"],
];
const BAR = "rgb(15 16 19 / .94)";
const PILL = {
  background: "rgb(22 22 22 / .92)",
  boxShadow: "inset 0 0 0 1px rgb(255 255 255 / .09)",
};

const paths = (list: string[]) => list.map((d) => <path key={d} d={d} />);

function Navigator() {
  const [busX, busY] = project(180, 260);
  return (
    <div className="nav-hud" style={{ width: 360 }}>
      <div
        className="flex items-baseline"
        style={{
          height: 34,
          padding: "0 11px",
          background: BAR,
          boxShadow: "inset 0 -1px rgb(255 255 255 / .09)",
        }}
      >
        <span
          style={{
            ...font(22, 700, "#ebebeb"),
            ...baseline(22, 23.7),
          }}
        >
          48
        </span>
        <span style={{ marginLeft: 4, ...font(14, 500, "#b2b2b2") }}>km/h</span>
        <span
          className="self-center"
          style={{ marginLeft: 8, width: 26, height: 26 }}
        />
        <span
          className="relative grid place-items-center self-center rounded-full"
          style={{
            marginLeft: 10,
            width: 21,
            height: 21,
            background: "#c82828",
          }}
        >
          <span
            className="grid place-items-center rounded-full"
            style={{
              width: 16.6,
              height: 16.6,
              background: "#ebebeb",
              ...font(11.5, 900, "#0f0f0f"),
              lineHeight: 1,
            }}
          >
            50
          </span>
        </span>
        <span className="ml-auto" style={font(12, 500, "#b2b2b2")}>
          Mon
        </span>
        <span style={{ marginLeft: 5, ...font(14, 700, "#ebebeb") }}>
          07:31
        </span>
      </div>
      <div
        className="relative overflow-hidden"
        style={{
          height: MAP.h,
          perspective: `${MAP.focal.toFixed(1)}px`,
        }}
      >
        <svg
          viewBox="-200 -300 760 720"
          className="absolute"
          style={{
            left: -200,
            top: -300 - MAP.shift,
            width: 760,
            height: 720,
            transformOrigin: `${MAP.w / 2 + 200}px ${MAP.h / 2 + 300 + MAP.shift}px`,
            transform: "rotateX(38deg)",
          }}
          aria-hidden="true"
        >
          <g fill="none" strokeLinecap="round" strokeLinejoin="round">
            <g stroke="rgb(30 30 30 / .9)" strokeWidth="15">
              {paths(ROADS)}
            </g>
            <g stroke="#5c5c5c" strokeWidth="11">
              {paths(ROADS.slice(2))}
            </g>
            <g stroke="#707070" strokeWidth="11">
              {paths(ROADS.slice(0, 2))}
            </g>
            <path d="M180 420 V140 H560" stroke="#2e74f0" strokeWidth="7" />
          </g>
          <g fill="#ecf4ff">{paths(CHEVRONS)}</g>
        </svg>
        {CARS.map(([x, y]) => (
          <Marker key={`${x},${y}`} x={x} y={y}>
            <Car />
          </Marker>
        ))}
        <Marker x={430} y={140}>
          <Stop
            r={6.5}
            fill="rgb(76 91 112 / .98)"
            ring={8}
            glyph={10.5}
            color="#ebebeb"
          />
        </Marker>
        <Marker x={290} y={140}>
          <Stop r={8} fill={ACCENT} ring={9.5} glyph={12.5} color="#120e08" />
        </Marker>
        <svg
          className="absolute"
          viewBox="-12 -12 24 24"
          style={{
            width: 24,
            height: 24,
            left: `${(busX - 12).toFixed(1)}px`,
            top: `${(busY - 12).toFixed(1)}px`,
          }}
          aria-hidden="true"
        >
          <path
            d="M0 -11.25 L-7.9 9 L0 4.5 L7.9 9 Z"
            fill="rgb(22 22 22 / .85)"
            strokeLinejoin="round"
          />
          <path d="M0 -9 L-6.3 7.2 L0 3.6 L6.3 7.2 Z" fill="#ebebeb" />
        </svg>
        <div
          className="absolute flex items-center rounded-md"
          style={{
            left: 8,
            top: 8,
            height: 34,
            paddingRight: 10,
            ...PILL,
          }}
        >
          <span className="grid place-items-center" style={{ width: 36 }}>
            <Icon name="turn_right" size={24} color={ACCENT} />
          </span>
          <span style={{ marginLeft: -2, ...font(14, 700, "#ebebeb") }}>
            150 m
          </span>
          <span style={{ marginLeft: 8, ...font(12, 500, "#b2b2b2") }}>
            Bahnhofstraße
          </span>
        </div>
        <span
          className="absolute left-1/2 -translate-x-1/2 rounded-full"
          style={{
            bottom: 6,
            padding: "0 7px",
            ...PILL,
            ...line(18, 11.5, 500, "#b2b2b2"),
          }}
        >
          Lindenallee
        </span>
      </div>
      <div
        style={{
          padding: "4px 11px 0",
          background: BAR,
          boxShadow: "inset 0 1px rgb(255 255 255 / .09)",
        }}
      >
        <div className="flex items-center" style={{ height: 20 }}>
          <span
            className="rounded"
            style={{
              height: 18,
              padding: "0 6px",
              background: ACCENT,
              ...font(12.5, 700, "#120e08"),
              lineHeight: "18px",
            }}
          >
            24
          </span>
          <span style={{ marginLeft: 7, ...font(13.5, 700, "#ebebeb") }}>
            Rathaus
          </span>
        </div>
        <div
          className="flex items-center justify-between whitespace-pre"
          style={{ height: 18 }}
        >
          <span style={font(12.5, 500, "#b2b2b2")}>
            {"850 m  ·  2 min  ·  07:33"}
          </span>
          <span style={font(12.5, 700, "#6ec878")}>on time</span>
        </div>
        <div
          style={{
            marginTop: 4,
            boxShadow: "inset 0 1px rgb(255 255 255 / .06)",
            padding: "6px 0 6px",
          }}
        >
          {NEXT.map(([planned, name, expected, color]) => (
            <div
              key={name}
              className="flex items-center"
              style={{ height: 22 }}
            >
              <span
                style={{
                  width: 46,
                  ...font(12.5, 700, "#b2b2b2"),
                }}
              >
                {planned}
              </span>
              <span className="flex-1" style={font(13, 500, "#ebebeb")}>
                {name}
              </span>
              <span style={font(12.5, 500, color)}>{expected}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export const DEMOS = [
  {
    title: "Choose a bus",
    text: "Buses detected in your OMSI 2 folder, sorted by manufacturer, with available liveries and a 3D preview.",
    Demo: Bus,
  },
  {
    title: "Pick a line and a tour",
    text: "Choose a line and a trip from the map's timetable.",
    Demo: Route,
  },
  {
    title: "Set the time and the weather",
    text: "Choose a date and weather, use live weather reports, or let conditions change as you drive.",
    Demo: Weather,
  },
  {
    title: "Drive with the navigator",
    text: "See your speed, upcoming turns and stops, and whether you are running late.",
    Demo: Navigator,
  },
];
