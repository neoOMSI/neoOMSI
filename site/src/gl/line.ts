import { mountShader, reducedMotion, vec3 } from "./shader";
import type { Theme } from "../lib/theme";

const MAX_POINTS = 12;
const SPEED = 170;
const DWELL = 1.6;

const PALETTES = {
  dark: {
    bg: "#0f0f0f",
    route: "#fd6b00",
    stop: "#0f0f0f",
    ring: "#fafafa",
    bus: "#fafafa",
  },
  light: {
    bg: "#ffffff",
    route: "#fd6b00",
    stop: "#ffffff",
    ring: "#18181b",
    bus: "#18181b",
  },
};

const fragment = (COLORS: (typeof PALETTES)["dark"]) => `
uniform vec2 u_points[${MAX_POINTS}];
uniform float u_count;
uniform float u_active;
uniform vec2 u_bus;
uniform vec2 u_heading;
uniform float u_dpr;

const vec3 BG = ${vec3(COLORS.bg)};
const vec3 ROUTE = ${vec3(COLORS.route)};
const vec3 STOP = ${vec3(COLORS.stop)};
const vec3 RING = ${vec3(COLORS.ring)};
const vec3 BUS = ${vec3(COLORS.bus)};

float segment(vec2 q, vec2 a, vec2 b) {
  vec2 qa = q - a;
  vec2 ba = b - a;
  return length(qa - ba * clamp(dot(qa, ba) / max(dot(ba, ba), 1e-4), 0.0, 1.0));
}

float fill(float d, float r) {
  return 1.0 - smoothstep(r - 0.75, r + 0.75, d);
}

void main() {
  vec2 q = vec2(gl_FragCoord.x, u_resolution.y - gl_FragCoord.y);
  float route = 1e4;
  float stop = 1e4;
  float active = 1e4;
  for (int i = 0; i < ${MAX_POINTS}; i++) {
    if (float(i) >= u_count) break;
    if (i > 0) route = min(route, segment(q, u_points[i - 1], u_points[i]));
    if (i > 0 && float(i) < u_count - 1.0) {
      float d = length(q - u_points[i]);
      stop = min(stop, d);
      if (float(i) == u_active) active = d;
    }
  }
  vec3 color = mix(BG, ROUTE, fill(route, 3.0 * u_dpr));
  color = mix(color, RING, fill(stop, 10.0 * u_dpr));
  color = mix(color, STOP, fill(stop, 5.5 * u_dpr));
  color = mix(color, ROUTE, fill(active, 5.5 * u_dpr));

  vec2 span = u_heading * 10.0 * u_dpr;
  float bus = segment(q, u_bus - span, u_bus + span);
  color = mix(color, BG, fill(bus, 10.5 * u_dpr));
  color = mix(color, BUS, fill(bus, 7.5 * u_dpr));

  gl_FragColor = vec4(color, 1.0);
}
`;

export function mountLine(
  canvas: HTMLCanvasElement,
  anchors: HTMLElement[],
  theme: Theme,
): () => void {
  const still = reducedMotion();
  let active = -1;

  const path = (canvas: HTMLCanvasElement, dpr: number) => {
    const box = canvas.getBoundingClientRect();
    const stops = anchors.map((a) => {
      const r = a.getBoundingClientRect();
      return [
        (r.left + r.width / 2 - box.left) * dpr,
        (r.top + r.height / 2 - box.top) * dpr,
      ] as [number, number];
    });
    const [first, last] = [stops[0], stops[stops.length - 1]];
    const vertical =
      stops.length > 1 &&
      Math.abs(stops[1][1] - first[1]) > Math.abs(stops[1][0] - first[0]);
    const w = box.width * dpr;
    const h = box.height * dpr;
    const start: [number, number] = vertical
      ? [first[0], 0]
      : [-30 * dpr, first[1]];
    const end: [number, number] = vertical
      ? [last[0], h]
      : [w + 30 * dpr, last[1]];
    return [start, ...stops, end];
  };

  const busAt = (points: [number, number][], seconds: number, dpr: number) => {
    const lengths = points
      .slice(1)
      .map((p, i) => Math.hypot(p[0] - points[i][0], p[1] - points[i][1]));
    const legs = lengths.map((l) => l / (SPEED * dpr));
    const period =
      legs.reduce((a, b) => a + b, 0) + DWELL * (points.length - 2);
    let t = still ? legs[0] : seconds % period;
    for (let i = 0; i < lengths.length; i++) {
      const [a, b] = [points[i], points[i + 1]];
      const heading = [(b[0] - a[0]) / lengths[i], (b[1] - a[1]) / lengths[i]];
      if (t <= legs[i]) {
        const k = t / legs[i];
        return {
          x: a[0] + (b[0] - a[0]) * k,
          y: a[1] + (b[1] - a[1]) * k,
          heading,
        };
      }
      t -= legs[i];
      if (i < lengths.length - 1) {
        if (t <= DWELL) return { x: b[0], y: b[1], heading };
        t -= DWELL;
      }
    }
    const last = points[points.length - 1];
    return { x: last[0], y: last[1], heading: [1, 0] };
  };

  const colors = PALETTES[theme];
  const shader = mountShader({
    canvas,
    fragment: fragment(colors),
    fallback: colors.bg,
    animate: true,
    resize(gl, uniform, { dpr }) {
      gl.uniform1f(uniform("u_dpr"), dpr);
    },
    frame(gl, uniform, seconds) {
      const dpr = canvas.width / Math.max(canvas.clientWidth, 1);
      const points = path(canvas, dpr);
      const flat = new Float32Array(MAX_POINTS * 2);
      points.slice(0, MAX_POINTS).forEach((p, i) => flat.set(p, i * 2));
      gl.uniform2fv(uniform("u_points"), flat);
      gl.uniform1f(uniform("u_count"), Math.min(points.length, MAX_POINTS));
      gl.uniform1f(uniform("u_active"), active + 1);
      const bus = busAt(points, seconds, dpr);
      gl.uniform2f(uniform("u_bus"), bus.x, bus.y);
      gl.uniform2f(uniform("u_heading"), bus.heading[0], bus.heading[1]);
    },
  });

  const links = anchors.map((a) => a.closest("a")!);
  const listeners = links.map((link, i) => {
    const enter = () => ((active = i), shader.draw());
    const leave = () => ((active = -1), shader.draw());
    link.addEventListener("pointerenter", enter);
    link.addEventListener("pointerleave", leave);
    link.addEventListener("focus", enter);
    link.addEventListener("blur", leave);
    return () => {
      link.removeEventListener("pointerenter", enter);
      link.removeEventListener("pointerleave", leave);
      link.removeEventListener("focus", enter);
      link.removeEventListener("blur", leave);
    };
  });
  document.fonts.ready.then(() => shader.draw());

  return () => {
    listeners.forEach((off) => off());
    shader.destroy();
  };
}
