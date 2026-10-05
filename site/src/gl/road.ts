import { mountShader, vec3 } from "./shader";
import type { Theme } from "../lib/theme";

type V3 = [number, number, number];

const TAN = 0.36;
const LIFT = 0.03;

const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const scale = (a: V3, k: number): V3 => [a[0] * k, a[1] * k, a[2] * k];
const cross = (a: V3, b: V3): V3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const norm = (a: V3) => scale(a, 1 / Math.hypot(...a));

const fragment = (bg: string) => `
uniform vec3 u_eye;
uniform vec3 u_forward;
uniform vec3 u_right;
uniform vec3 u_up;
const vec3 BG = ${vec3(bg)};
const vec3 LINE = ${vec3("#fd6b00")};

float stripe(float x, float centre, float w) {
#ifdef HAS_DERIVATIVES
  float aa = fwidth(x);
#else
  float aa = 0.02;
#endif
  return 1.0 - smoothstep(w - aa, w + aa, abs(x - centre));
}

void main() {
  vec2 ndc = gl_FragCoord.xy / u_resolution * 2.0 - 1.0;
  vec3 ray = normalize(u_forward + ndc.x * ${TAN} * (u_resolution.x / u_resolution.y) * u_right + ndc.y * ${TAN} * u_up);
  vec3 color = BG;
  if (ray.y < 0.0) {
    vec3 p = u_eye - ray * (u_eye.y / ray.y);
    float lines = stripe(p.z, 2.25, 0.03) + stripe(p.z, -5.6, 0.03) + stripe(p.z, -1.85, 0.05) * step(fract(p.x / 9.0), 0.34);
    float fade = (1.0 - smoothstep(4.0, 12.0, abs(p.z + 1.6))) * (1.0 - smoothstep(30.0, 70.0, abs(p.x)));
    color = mix(BG, LINE, clamp(lines, 0.0, 1.0) * fade);
  }
  gl_FragColor = vec4(color, 1.0);
}
`;

function camera(aspect: number) {
  const wide = aspect > 1.15;
  const fit = (half: number) => half / (TAN * Math.min(aspect, 1.9));
  const back = norm([0.95, 0.16, 0.62]);
  const dist = wide ? Math.max(fit(10.5), 14) : Math.max(fit(4.6), 12);
  const height = 2 * dist * TAN;
  const side = norm(cross(scale(back, -1), [0, 1, 0]));
  const target = add(
    add([2.4, 1.5, 0], scale(side, wide ? -0.05 * height * aspect : 0)),
    [0, ((wide ? 0.17 : 0.22) - LIFT) * height, 0],
  );
  const forward = scale(back, -1);
  const right = norm(cross(forward, [0, 1, 0]));
  return {
    eye: add(target, scale(back, dist)),
    forward,
    right,
    up: cross(right, forward),
  };
}

export function mountRoad(canvas: HTMLCanvasElement, theme: Theme): () => void {
  const bg = theme === "light" ? "#ffffff" : "#0f0f0f";
  return mountShader({
    canvas,
    fragment: fragment(bg),
    fallback: bg,
    derivatives: true,
    resize(gl, uniform, { width, height }) {
      const view = camera(width / height);
      gl.uniform3fv(uniform("u_eye"), view.eye);
      gl.uniform3fv(uniform("u_forward"), view.forward);
      gl.uniform3fv(uniform("u_right"), view.right);
      gl.uniform3fv(uniform("u_up"), view.up);
    },
  }).destroy;
}
