# Manga SFX Maker: Project Notes

Native Rust desktop app (egui/eframe, no web) that turns typed text into editable vector manga sound-effect lettering. This file is the living record of architecture, status, and how to extend it. **Update it at the end of every milestone.**

## Current status

| Milestone | State |
|-----------|-------|
| M1: window, font load, per-letter outlines, pan/zoom | **Done** (build + clippy + tests green) |
| M2: selection, move/rotate/scale, undo/redo | Not started |
| M3: PNG + SVG export | Not started |
| M4: Params, sliders, layout, seeded effects | Not started |
| M5: guide-path warp | Not started |
| M6: fills, tones, outlines | Not started |
| M7: presets, Roll, Mutate | Not started |
| M8: liquify | Not started |
| M9: brush lettering, ZIP export, polish | Not started |

## Changelog

### M1
- Cargo project `sfx_maker`, eframe with the **glow** backend (not wgpu: faster to compile, fewer driver issues; we only upload one texture per frame).
- `FontData` (validated font bytes + face index), `text_to_paths` (string -> one `BezPath` per visible glyph, horizontal/vertical, multi-line).
- `render_scene` (tiny-skia) draws letters black on white; result is uploaded as an egui texture.
- UI: multiline text box, direction radio, "Open font file…" (rfd), drag-and-drop of .ttf/.otf/.ttc, system font picker (fontdb), Fit view, status bar for errors.
- Pan = drag, zoom = wheel/pinch around cursor.
- Bundled font: Bangers (OFL), see `assets/fonts/LICENSES.md`.
- Tests: glyph layout, empty text, bad font returns an error, determinism, renderer produces ink.

## Building (important on this machine)

The default rustup toolchain here (`stable-x86_64-pc-windows-gnu`) cannot link (llvm-mingw has no libgcc) and MSVC Build Tools are not installed. A **gnullvm** toolchain works:

```powershell
$env:Path += ";$env:USERPROFILE\.cargo\bin"
cargo +stable-x86_64-pc-windows-gnullvm run
cargo +stable-x86_64-pc-windows-gnullvm clippy --all-targets
cargo +stable-x86_64-pc-windows-gnullvm test
```

On macOS/Linux, or with MSVC installed, plain `cargo run` is fine. No platform-specific code exists in the project.

## Architecture

```
src/
  main.rs               eframe entry, window options
  app.rs                SfxApp: panels, input, canvas texture caching
  model/                pure data (serde-serializable), no UI/font handles
    letter.rs           Letter { ch, glyph_path (local space, y-down), transform }
    scene.rs            Scene { text, direction, letters, seed }
  geometry/             pure functions: paths in -> paths out (unit-testable, thread-safe)
    text_to_paths.rs    FontData, text_to_paths, EM_SIZE
  render/
    raster.rs           render_scene(scene, view, w, h, background) -> Pixmap
assets/fonts/           bundled OFL fonts + LICENSES.md
```

Planned (per spec): `model/params.rs`, `model/preset.rs`, `geometry/{warp,liquify,outline,effects}.rs`, `render/tones.rs`, `export/{png,svg}.rs`, `random/gacha.rs`.

### Key conventions
- **Coordinates:** scene space is y-down. `EM_SIZE = 200` scene units. Glyph paths are stored at the origin; `Letter.transform` positions them. Font y-up is flipped in `PathSink`.
- **One renderer:** the canvas and (later) PNG export call `render_scene`; they differ only in the `view` affine and background. Keep it that way so screen = export.
- **View transform** in `app.rs`: `scale(ppp) * translate(pan) * scale(zoom)`. `pan` is in canvas-local points.
- **Re-render gating:** `RenderKey` (size, pan, zoom, `scene_version`). **Any code that mutates the scene must bump `scene_version`** (see `relayout`) or the canvas won't update.
- **Errors:** no `unwrap()` on user paths. Errors go through `SfxApp::set_error` into the status bar.
- **Determinism:** seed lives in `Scene.seed`. Plan: per-letter RNG `ChaCha8Rng` seeded from `seed ^ letter_index` so results don't depend on thread order.

## How to modify

- **Add a geometry effect:** new fn in `geometry/` taking `&BezPath`/params and returning a new path; add unit tests; call it from the scene-build step (arrives in M4) rather than from UI code.
- **Add a UI control:** add the field to `model/`, edit it in `SfxApp::side_panel`, call `relayout()` (or bump `scene_version` if only visual).
- **Add a font:** drop an OFL file in `assets/fonts/`, add a row to `LICENSES.md`.
- **egui 0.37 API notes** (differs from older tutorials): `eframe::App` implements `ui(&mut self, ui, frame)` (not `update`); panels are `egui::Panel::left(..).show(ui, ..)`; `DroppedFile` is a trait (`path()`, `bytes()`).

## Known limitations / TODO
- No kerning yet (planned with optical spacing, M4).
- System fonts load synchronously at startup (fontdb scan); move to a worker thread if slow.
- File dialog is synchronous on the UI thread (fine for M1).
- Only the first face of a .ttc is used.
