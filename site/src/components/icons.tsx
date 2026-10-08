import type { CSSProperties } from "react";
import { siAndroid, siApple, siLinux } from "simple-icons";

const svgs = import.meta.glob<string>(
  "../../../assets/icons/material/{download,description,directions_bus,search,star,check,chevron_right,chevron_left,expand_more,event,wb_sunny,tune,public,autorenew,calendar_month,turn_right,sync_alt,speed,monitor,install_desktop,departure_board,dns,extension,check_circle,info,bolt,error,warning,content_copy,chat,history,menu,arrow_back,open_in_new,link,light_mode,dark_mode,nights_stay,expand_less,construction,alt_route,flag,inventory_2}.svg",
  { query: "?raw", import: "default", eager: true },
);

const PATHS: Record<string, string> = Object.fromEntries(
  Object.entries(svgs).map(([file, svg]) => [
    file.match(/(\w+)\.svg$/)![1],
    svg.match(/ d="([^"]+)"/)![1],
  ]),
);

const WINDOWS =
  "M3 3h8.4v8.4H3zM12.6 3H21v8.4h-8.4zM3 12.6h8.4V21H3zM12.6 12.6H21V21h-8.4z";

interface Props {
  name: string;
  size?: number;
  color?: string;
  style?: CSSProperties;
}

const box = (
  size: number,
  color?: string,
  style?: CSSProperties,
): CSSProperties => ({
  width: size,
  height: size,
  flex: "none",
  color,
  ...style,
});

export function Icon({ name, size = 18, color, style }: Props) {
  const d = PATHS[name];
  if (!d) throw new Error(`Missing icon ${name}`);
  return (
    <svg
      viewBox="0 -960 960 960"
      fill="currentColor"
      aria-hidden="true"
      style={box(size, color, style)}
    >
      <path d={d} />
    </svg>
  );
}

export function iconMarkup(name: string, size = 18) {
  return `<svg viewBox="0 -960 960 960" fill="currentColor" aria-hidden="true" style="width:${size}px;height:${size}px;flex:none"><path d="${PATHS[name]}"/></svg>`;
}

const BRANDS: [string, string][] = [
  ["windows", WINDOWS],
  ["mac", siApple.path],
  ["android", siAndroid.path],
  ["linux", siLinux.path],
];

export function PlatformIcon({
  build,
  size = 18,
}: {
  build?: string;
  size?: number;
}) {
  const path = build && BRANDS.find(([key]) => build.includes(key))?.[1];
  if (!path) return <Icon name="download" size={size} />;
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
      style={box(size)}
    >
      <path d={path} />
    </svg>
  );
}
