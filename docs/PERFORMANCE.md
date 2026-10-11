# Renderer performance checks

`cargo run --profile dev-release -p render --example frame_cost` runs a synthetic CPU
benchmark through wgpu's noop backend. It needs no game content or graphics adapter.
It measures mesh upload preparation, rendering a large scene with a mirror across a
floating-origin cell boundary, preparing distant shadow casters, and scattered transform
and material updates. These are CPU measurements, not game FPS or GPU timings; compare
the same build profile on the same
machine with other workloads stopped

The renderer keeps these costs bounded without changing graphics settings:

- Nearby mirrors, the second stereo eye and side panels reuse the primary view's
  floating origin. Previously each view snapped its own camera to a 100 m cell. A
  mirror across the cell boundary invalidated and rebuilt every instance's model and
  material parameters, even for static objects outside the picture. The primary view
  still rebases normally; distant secondary cameras also rebase to preserve precision.
- Pending animated meshes are indexed by mesh ID. Updating all `N` meshes previously
  performed roughly `N * (N - 1) / 2` ID comparisons. The indexed queue replaces the
  last pending pose and retains its table capacity after submission.
- Shadow preparation applies the existing geometric rejection tests before examining
  material slots. Objects outside all active sun and spotlight views need no material
  classification. The geometric thresholds, alpha rules and submitted draws are unchanged.
- Incremental instance uploads compare model matrices and material parameters separately.
  Lighting changes upload parameters; motion uploads matrices, plus parameters if needed
  (for example, moving a roof height used for lighting). Merged writes copy at most as much
  unchanged padding as changed data, instead of bridging a chain of 4,096-slot gaps.
- Tile workers resolve texture paths before decoding and compressing them. Different
  search-folder lists that resolve to one file share one decode within the batch, reducing
  CPU work and temporary image memory. Different pack files retain their own identities.
- The 1 MB light grid is kept between frames. Only cells that hold a light now, or held
  one before, are rewritten, and only the changed span is uploaded. Previously the grid
  was rebuilt, compared and, once a headlight moved, uploaded in full for the picture and
  for each mirror.
- Animated meshes (cars, people, the driver) are uploaded through one staging buffer per
  frame and copied before the first pass. wgpu's `write_buffer` creates a staging buffer
  per call, so one call per mesh meant one driver allocation per mesh per frame.
- The main pass is recorded in up to four parts, each finished on an encoding thread;
  culling and shadow-caster collection split the scene into parts of 4,096 objects.
  Parts keep the draw order, so the picture is unchanged.
- Script programs answer `stores(var)` from a set built once, and name lookups no longer
  allocate a lower-case copy. Passengers and the driver figure query a bus's door
  variables several times a frame; each query used to scan every op of every block.
- The window picture's command buffers are finished, submitted and presented on a thread of
  their own (`render-submit`), while the main thread goes on with the next frame's traffic,
  vehicle and people. `finish`, `submit` and `present` took about a quarter of the main
  thread's frame. Every write to a buffer a picture reads happens in `render_inner`, which
  waits for the thread first. `OMSI_NO_SUBMIT_THREAD=1` submits on the main thread again.
- The mirrors and the navigator panel leave their commands for the same submission, ahead of
  the picture's own. The navigator used a submission of its own for each pass, about 1 ms
  each time the panel was redrawn; the interface's layer uniforms now take slots of their own
  per draw, so its passes share one encoder.
- A picture's buffer writes (camera, lights, light grid, instance ranges, post uniforms) are
  gathered into one staging buffer and copied by a command buffer submitted just before the
  picture's. `write_buffer` made a staging buffer per call, some 25 a picture.

On Windows, DirectX 12 compiles shaders with DXC when `dxcompiler.dll` and `dxil.dll`
sit next to `neoomsi.exe`, as release packages ship them. Without them wgpu falls back
to FXC, and the first start takes about 55 s instead of about 16 s on a fast CPU.
Local builds in `target/` have no DXC: copy the two DLLs there to start as fast as a release.
Compiling the pipeline variants on several threads does not help. DXC and FXC serialize
the compiles internally, and parallel cache misses compile shared stages several times.

For report of very low FPS, run the game with `OMSI_PROFILE=1`, `RUST_LOG=info`, and
`--exit-after 120` using the reported map, vehicle, view and traffic settings. The exit
summary records CPU stage times, draw counts, tile streaming spikes, incremental instance
upload bytes/write counts, and GPU pass times where the adapter supports them
(`OMSI_GPU_TIMERS=1`). With `--exit-after`, stage averages cover only the second half of
the run, after loading. `frame.*` entries time every phase of the frame without gaps.
`profile thread` lines list each thread's CPU time per frame.

Dev builds (`dev-release` or `dev` with the default `devtools` feature) on Windows can
sample the main thread to show which functions the stages spend their time in, without an
elevated profiler: `OMSI_SAMPLE=<start s>,<length s>` (default `40,40`) stops the main
thread every millisecond, reads its call stack and, after the length, writes
`sample-report.txt` to the working folder: the functions by their own samples, with what
they call, and caller → callee pairs, named from the PDB. Start sampling after loading,
e.g. `OMSI_SAMPLE=40,40` with `--exit-after 90`.
Include the map/vehicle, graphics mode, mirror/VR
settings, adapter, and whether the problem occurs during steady driving or loading.
Do not infer that one improved CPU benchmark explains every low-FPS report: streaming,
scripts, GPU fill cost and memory pressure require their own reproduction and measurements.
