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

For report of very low FPS, run the game with `OMSI_PROFILE=1`, `RUST_LOG=info`, and
`--exit-after 120` using the reported map, vehicle, view and traffic settings. The exit
summary records CPU stage times, draw counts, tile streaming spikes, incremental instance
upload bytes/write counts, and GPU pass times where the adapter supports them.
Include the map/vehicle, graphics mode, mirror/VR
settings, adapter, and whether the problem occurs during steady driving or loading.
Do not infer that one improved CPU benchmark explains every low-FPS report: streaming,
scripts, GPU fill cost and memory pressure require their own reproduction and measurements.
