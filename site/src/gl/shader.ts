export type Uniform = (name: string) => WebGLUniformLocation | null;
export type Size = { width: number; height: number; dpr: number };

export interface ShaderSpec {
  canvas: HTMLCanvasElement;
  fragment: string;
  fallback: string;
  derivatives?: boolean;
  animate?: boolean;
  resize?: (gl: WebGLRenderingContext, uniform: Uniform, size: Size) => void;
  frame?: (
    gl: WebGLRenderingContext,
    uniform: Uniform,
    seconds: number,
  ) => void;
}

export interface Shader {
  draw: () => void;
  destroy: () => void;
}

const VERTEX =
  "attribute vec2 a_position; void main() { gl_Position = vec4(a_position, 0.0, 1.0); }";

export const reducedMotion = () =>
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function rgb(hex: string): [number, number, number] {
  const value = parseInt(hex.slice(1), 16);
  return [
    ((value >> 16) & 0xff) / 255,
    ((value >> 8) & 0xff) / 255,
    (value & 0xff) / 255,
  ];
}

export function vec3(color: string): string {
  return `vec3(${rgb(color)
    .map((c) => c.toFixed(4))
    .join(", ")})`;
}

function header(gl: WebGLRenderingContext, derivatives?: boolean) {
  const highp = gl.getShaderPrecisionFormat(gl.FRAGMENT_SHADER, gl.HIGH_FLOAT);
  const extension =
    derivatives && gl.getExtension("OES_standard_derivatives")
      ? "#extension GL_OES_standard_derivatives : enable\n#define HAS_DERIVATIVES\n"
      : "";
  return `${extension}precision ${highp && highp.precision > 0 ? "highp" : "mediump"} float;
uniform vec2 u_resolution;
`;
}

function compile(gl: WebGLRenderingContext, type: number, source: string) {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (gl.getShaderParameter(shader, gl.COMPILE_STATUS)) return shader;
  console.warn(gl.getShaderInfoLog(shader));
  gl.deleteShader(shader);
  return null;
}

function link(gl: WebGLRenderingContext, fragment: string) {
  const vs = compile(gl, gl.VERTEX_SHADER, VERTEX);
  const fs = compile(gl, gl.FRAGMENT_SHADER, fragment);
  const program = vs && fs ? gl.createProgram() : null;
  if (!program || !vs || !fs) return null;
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  if (gl.getProgramParameter(program, gl.LINK_STATUS)) return program;
  gl.deleteProgram(program);
  return null;
}

export function mountShader(spec: ShaderSpec): Shader {
  const { canvas } = spec;
  const parent = canvas.parentElement!;
  const noop: Shader = { draw() {}, destroy() {} };

  const fail = () => {
    canvas.width = parent.clientWidth;
    canvas.height = parent.clientHeight;
    const ctx = canvas.getContext("2d");
    if (ctx) {
      ctx.fillStyle = spec.fallback;
      ctx.fillRect(0, 0, canvas.width, canvas.height);
    }
    return noop;
  };

  const gl = canvas.getContext("webgl", {
    antialias: false,
  }) as WebGLRenderingContext | null;
  if (!gl) return fail();
  const program = link(gl, header(gl, spec.derivatives) + spec.fragment);
  if (!program) return fail();
  gl.useProgram(program);

  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 3, -1, -1, 3]),
    gl.STATIC_DRAW,
  );
  const position = gl.getAttribLocation(program, "a_position");
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);

  const uniform: Uniform = (name) => gl.getUniformLocation(program, name);
  const resolution = uniform("u_resolution");

  let size: Size = { width: 0, height: 0, dpr: 1 };
  let seconds = 0;

  const draw = () => {
    if (!size.width || !size.height) return;
    spec.frame?.(gl, uniform, seconds);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };

  const resize = () => {
    const width = parent.clientWidth;
    const height = parent.clientHeight;
    if (!width || !height) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    size = { width: canvas.width, height: canvas.height, dpr };
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform2f(resolution, canvas.width, canvas.height);
    spec.resize?.(gl, uniform, size);
    draw();
  };
  resize();
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(parent);

  const animate = spec.animate && !reducedMotion();
  let raf = 0;
  let previous = 0;
  const loop = (now: number) => {
    if (previous) seconds += (now - previous) / 1000;
    previous = now;
    draw();
    raf = requestAnimationFrame(loop);
  };
  const intersectionObserver = new IntersectionObserver(([entry]) => {
    if (entry.isIntersecting && animate && !raf) {
      previous = 0;
      raf = requestAnimationFrame(loop);
    } else if (!entry.isIntersecting && raf) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
  });
  intersectionObserver.observe(canvas);

  return {
    draw,
    destroy() {
      resizeObserver.disconnect();
      intersectionObserver.disconnect();
      cancelAnimationFrame(raf);
      gl.deleteBuffer(buffer);
      gl.deleteProgram(program);
    },
  };
}
