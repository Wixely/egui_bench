# gcad renderer fork

Reviewed: 2026-11-02. Maintainer: Wixely. Review on upstream updates.

This branch starts at upstream commit
`108d93108fbe89d07a526bc51e3dd92688d87eb5`, the source of the published
`egui_bench` 0.9.2 crate. It carries gcad's previously vendored renderer changes:

- Validated explicit mip chains and correct sRGB/numeric sampling.
- Opaque, masked and blended material alpha.
- Occlusion and emissive maps.
- Shared UV scale/offset for GPU raster material maps.
- Renderer regression tests for the added paths.

Default values retain the existing material behavior. Software rendering and
tracing explicitly reject unsupported new material paths. Existing MIT and font
OFL license notices remain unchanged. This branch does not publish a crates.io
release; consumers should pin its exact Git revision.

## Validation

From a gcad development checkout, generate a fresh synthetic atlas fixture:

```sh
cargo run --locked --features game-assets --example game_asset_renderer_fixture -- target/fork-atlas.bin
```

From this fork, run its renderer suite with that fixture's absolute path:

```sh
GCAD_ATLAS_FIXTURE=/absolute/path/to/fork-atlas.bin GCAD_RENDER_BACKEND=dx12 \
  cargo test --locked --features viewer3d --lib viewer3d:: -- --test-threads=1
GCAD_ATLAS_FIXTURE=/absolute/path/to/fork-atlas.bin GCAD_RENDER_BACKEND=vulkan \
  cargo test --locked --features viewer3d --lib viewer3d:: -- --test-threads=1
```

Tests assert the requested backend; a fallback is not acceptance. The atlas
fixture contains synthetic geometry and pixels, not user assets. Keep generated
fixtures and test output outside commits. Also validate the consuming gcad build
and game-asset GPU tests when updating the dependency revision.

Future renderer changes belong here. Rebase or merge upstream changes explicitly,
rerun validation, then update gcad's revision and lockfile together. Do not modify
an old vendored copy and assume the fork will receive those changes.
