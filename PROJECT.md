# Manga SFX Maker: Project Notes

Native Rust desktop app (egui/eframe, no web) that turns typed text into editable vector manga sound-effect lettering. This file is the living record of architecture, status, and how to extend it. **Update it at the end of every milestone.**

## Current status

| Milestone | State |
|-----------|-------|
| M1: window, font load, per-letter outlines, pan/zoom | **Done** (build + clippy + tests green) |
| M2: selection, move/rotate/scale, undo/redo | **Done** (clippy + 21 tests green; interaction not yet hand-tested in the GUI) |
| M3: PNG + SVG export | **Done** (unit-tested; file dialogs not yet hand-tested) |
| M4: Params, sliders, layout, seeded effects | **Done** (clippy + 25 tests green; sliders not yet hand-tested in the GUI) |
| M5: guide-path warp | **Done** (unit-tested; guide editing not yet hand-tested in the GUI) |
| M6: fills, tones, outlines | **Skipped** (by request) |
| M7: presets, Roll, Mutate | **Done** (unit-tested; UI not yet hand-tested) |
| M8: liquify | **Skipped** (by request) |
| M9: brush lettering, ZIP export, polish | **Done** (clippy + 48 tests green; interactive parts not yet hand-tested) |

## Changelog

### M1
- Cargo project `sfx_maker`, eframe with the **glow** backend (not wgpu: faster to compile, fewer driver issues; we only upload one texture per frame).
- `FontData` (validated font bytes + face index), `text_to_paths` (string -> one `BezPath` per visible glyph, horizontal/vertical, multi-line).
- `render_scene` (tiny-skia) draws letters black on white; result is uploaded as an egui texture.
- UI: multiline text box, direction radio, "Open font file…" (rfd), drag-and-drop of .ttf/.otf/.ttc, system font picker (fontdb), Fit view, status bar for errors.
- Pan = drag, zoom = wheel/pinch around cursor.
- Bundled font: Bangers (OFL), see `assets/fonts/LICENSES.md`.
- Tests: glyph layout, empty text, bad font returns an error, determinism, renderer produces ink.

### M2
- `interact.rs` (pure, tested): `hit_letter` (exact glyph hit, falls back to smallest bbox so thin/hollow letters are clickable), `selection_bounds`, `scale_about`, `rotate_about`.
- `history.rs`: generic snapshot `History<T>` (200 deep). App stores whole-`Scene` snapshots; `record()` must be called BEFORE mutating.
- Selection (`BTreeSet<usize>`): click, Shift+click toggles, Ctrl+A, Esc. Drag a letter = move; corner handles = uniform scale about opposite corner; round handle above box = rotate about centre (Shift = 15° snap). Drag on empty space still pans.
- Drags are derived from `DragOp.orig` transforms (no drift); one undo step per drag. Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z, plus buttons. Shortcuts are skipped while a text field has focus.
- Selection overlay is painted by egui, never by the rasterizer, so exports exclude it.

### M3
- `export/png.rs`: `render_export` reuses `render_scene` with a fit-to-bounds view (4% padding), longest side 256-8192 px, transparent or white background; saved with tiny-skia `save_png`.
- `export/svg.rs`: one `<path id="letter-N">` per letter in a group, viewBox = padded bounds, optional white `<rect>`.
- `Scene::bounds()` is the shared bounds helper (view fit, export).
- UI: size slider, transparent checkbox, Export PNG/SVG buttons (rfd save dialog); result in status bar (green notice / red error). SVG uses the same "transparent" checkbox (unchecked = white rect).

### M4
- `model/params.rs`: `Params` (tracking, line spacing, kerning, rotation/scale/offset jitter, baseline wobble, size ramp, slant); `Scene.params`. Defaults are neutral.
- `text_to_paths_with(font, text, dir, &Params)` applies tracking, line spacing and legacy `kern`-table kerning (horizontal only; no GPOS). `text_to_paths` is now a default-params wrapper (tests only).
- `geometry/effects.rs`: `apply_effects` runs after layout; each letter gets its own `ChaCha8Rng` from `seed ^ index*golden`, so results are order-independent. Effects act about each glyph's bbox centre. Every letter consumes the same draws, so moving one slider never reshuffles others.
- UI: "Layout & effects" sliders, seed drag value, dice (new seed), Reset. Any change calls `relayout()` (manual transforms reset); a whole slider drag is one undo step (`param_snapshot`).

### M5
- `geometry/warp.rs`: `Guide` (polyline + arc-length sampling), `catmull_rom`, `procedural_guide` (from `Params.bend` / `wave` / `wave_cycles`), `warp_letters`. Glyphs are flattened and long lines subdivided, then each point maps (flow axis -> arc length, baseline offset -> normal offset). Warped letters get an identity transform and polyline outlines.
- `geometry/build.rs::build_text_letters` is the single scene-build step: layout -> effects -> warp. A user guide (`Scene.guide`, control points through a Catmull-Rom curve) overrides the procedural one. Text extent is stretched to the guide length.
- UI: "Draw custom guide" creates a straight 4-point guide on the baseline; with "Edit guide points": click adds, drag moves, right-click removes (min 2). One undo step per drag-start/click.

### M7
- `model/preset.rs`: `Preset { name, params }`, built-ins in code, user presets in `<config dir>/sfx_maker/presets.json` (corrupt/missing file = no presets; old files load thanks to `#[serde(default)]` on `Params`).
- `random/gacha.rs`: `roll` (random look from a seeded RNG, new seed) and `mutate` (nudges each field by up to `amount` of its slider range). Slider ranges live in `params::ranges`, shared by UI, clamping and the generators.

### M10: Giongo-Style Transforms & Letter Gizmo
- **Giongo On-Canvas Gizmo**: Clicking a letter selects it and renders the exact Giongo letter gizmo:
  - Cyan glowing contour outline around glyph silhouettes (`#00CFD8` stroke with white halo backing).
  - Blue sideways arrow (`#75D9FF`, X-axis move).
  - Yellow up/down arrow (`#F2C94C`, Y-axis move).
  - White center square & coral quadrant patch (`#FFFFFF`, free 2D move).
  - Coral rotate ring (`#FF8C73`, rotate about centre; Shift snaps to 15° increments).
  - Silver diagonal handle (`#D8D8D8`, uniform scale about centre).
  - Bottom-right HUD legend (`— Sideways   ↑ Up/down   □ Free   ○ Rotate   ■ Scale`) visible whenever letters are selected.
  - Double-click reset: double clicking a letter or gizmo handle resets its transform and distort corners.
- **English SFX Dictionary & Phonetic Auto-Pick**: 8 genres with phonetic character analysis; 0 Japanese dependencies.
- **FontLab 8 Simple (TransType/Fontographer) Weight Algorithm**: `weight_stem_ratio`, `weight_keep_v`, `weight_keep_h`, and `weight_round`.
- **Giongo-Style Transform Panel**: None, Trapezoid Skew (`trap_h`, `trap_v`), and Grid Presets with `roll_warp`.
- `export/project.rs`: save/open the whole `Scene` as JSON (`serde_json` with `float_roundtrip`). Open does not need the original font; later text edits use the current font.
- Polish: Copy PNG to clipboard (arboard, `pixmap_to_rgba` un-premultiplies), arrow-key nudge (Shift = 10 units).
- App code for these tools lives in `app/tools.rs` (child module of `app`, so it can use `SfxApp` privates).

### Library, font folders, copy
- **No system fonts.** `fontdb` is gone. `fonts.rs` scans user-chosen folders (recursive, 3 levels; .ttf/.otf/.ttc, first face only); the bundled Bangers is always the first entry (`library_ui::BUNDLED_NAME`). Folders persist in `<config dir>/sfx_maker/settings.json` (`settings.rs`). Dropping/opening a single font file still works.
- **Library** (`library.rs` + `app/widgets.rs       scroll-aware sliders
  app/library_ui.rs`, leftmost panel): `<data dir>/sfx_maker/library/<font name>/<sfx name>.sfx.json`. The name comes from the SFX text (sanitised for Windows, max 40 chars, `BOOM`, `BOOM 2`, ...; empty text = `SFX`). Files hold `{font, scene}`; opening one restores the scene (letters included) and switches to that font if it is in the font list. Empty folders disappear when their last entry is deleted.
- **Copy PNG** is the big button at the top of the controls and `Ctrl+C` (when no text field has focus). Uses the Export size / Transparent settings (capped at 4096 px).

### GPU canvas, scroll sliders, centring, right-hand library
- **GPU canvas** (`render/gpu.rs`): glyphs are tessellated with lyon (non-zero fill) into one triangle mesh, uploaded once, and drawn by an OpenGL pass through an `egui_glow` paint callback (own GLSL program; picks 120/140/ES variants via `ShaderVersion`). Pan/zoom are two uniforms, so panning costs no CPU. Re-tessellation happens only when `scene_version` or the zoom bucket (power of two of zoom*ppp) changes. 4x MSAA is requested in `main.rs`. Shader/link failures show in the status bar. **PNG/SVG export and Copy PNG still use the CPU rasterizer** (`render/raster.rs`), so exports never depend on the GPU; AA differs slightly from the canvas.
- **Scroll sliders** (`app/widgets.rs`): every slider reacts to the wheel while hovered (one notch = 2% of range, wheel up = increase) and swallows that scroll so the panel doesn't scroll too. Use `widgets::slider` / `slider_u32` for new sliders.
- **Centring**: `relayout()` fits the view (text/direction/font), `relayout_center()` re-centres keeping zoom (sliders, presets, guide create/clear; refits if it no longer fits), `relayout_static()` leaves the view alone (guide point drags). The content also stays centred when the canvas resizes (library toggle, window resize). Manual letter moves never move the view.
- **Library** is the right panel; hide/show with the status-bar button, the panel's ✕, or Ctrl+L.

### Free transform, warp grid, persistent manual edits
- **Free Transform** (``app/transform.rs``, Photoshop-like): 8 handles. Corners scale non-proportionally (Shift = proportional, Alt = from centre, dragging past the pivot flips); side handles scale one axis; **Ctrl + side handle = skew**; rotate with the round handle or by dragging just outside a corner (Shift = 15° steps); dragging anywhere inside the selection box moves it. Cursor icons follow the handle under the pointer. Pure maths (``scale_transform``, ``skew_transform``) is unit-tested.
- **Distort / perspective** (Ctrl + corner handle; Alt as well = perspective, neighbouring corners move the opposite way): acts only on the **selected letters**. Each letter stores ``Letter.distort`` (offsets of its local bounding-box corners, as fractions of its size; ``geometry/distort.rs``), applied before ``transform`` via ``Letter::local_path()``; so renderers/hit-testing use ``local_path`` / ``placed_path``, never ``glyph_path`` directly. A multi-letter selection is distorted as a group quad (each letter's corners are mapped through the group's bilinear map). Distort is part of ``ManualDelta`` so it survives relayouts; "Reset moved letters" clears it. Mapping is bilinear, not projective.
- **Slider wheel**: ``widgets::over_row`` treats the whole slider row (rail, value box, label) as hover area; ``Response::hovered`` alone only covered the rail. Headless egui tests (``app::widgets::tests``, ``app::tests``) drive the real side panel with wheel events.
- **Warp grid** (``geometry/free_warp.rs``): Edit > Transform > Warp equivalent. A 4x4 Bezier control grid over the text bounds (after layout, effects and guide); drag any point, or pick a style (Arc, Arch, Bulge, Flag, Rise, Squeeze) + bend slider. Offsets are stored as fractions of the bounds (``Scene.free_warp``), so the warp follows the text when sliders change the layout. Applied in ``build_text`` after the guide warp; ``Reset warp`` clears it. Warped outlines are polylines.
- **Manual edits survive relayout**: ``Letter.base`` records the pipeline transform; ``Letter::manual_delta`` / ``apply_manual`` store a manual move/scale/rotate/skew as a delta about the letter's own centre and re-apply it after every relayout (sliders, presets, Roll/Mutate, guide/warp edits). Only text/direction/font changes (count of letters changes meaning) reset them; "Reset moved letters" does it explicitly. Files saved before ``base`` existed load with no recorded edit.
- **Smoother bends**: warped outlines are flattened at 0.04 units and split into <=1.5 unit segments, the procedural guide uses 400 samples and ``Guide::sample`` interpolates vertex tangents, which removes the jagged edges seen at strong bend.
- **Roll / Mutate never change wave / wave cycles** (Roll copies them from the current look).

### Stroke weight (thin / thick, start / end)
- ``geometry/weight.rs``: outline offsetting. The glyph is flattened, every point moves along its mitred normal (limit 2.5x) by ``offset_at(params, t)`` = ``(weight + lerp(weight_start, weight_end, t)) * EM``, then ``i_overlay`` cleans it with the ``Positive`` fill rule so thinning past a stroke's width removes the stroke instead of leaving inverted garbage. Holes (counters) move the opposite way automatically, so thickening closes them.
- ``t`` runs along the flow axis (x horizontal, y vertical) over the whole text, or over each letter with ``weight_per_letter``. Pipeline order: layout -> effects -> **weight** -> guide warp -> free warp, so bends / warps act on the already-weighted shape.
- Params: ``weight``, ``weight_start``, ``weight_end`` (em, -0.08..0.12), ``weight_per_letter``. Weighted letters become polylines with an identity transform (like warped ones). Not touched by Roll / Mutate.
- **Uniform `weight` now uses `uniform_offset`** (i_overlay `OutlineOffset`, mitred joins, true polygon offset), so thinning/thickening keeps shapes intact. Only the start->end ramp still uses the per-vertex normal approximation (`offset_path`), applied on top of the exact result.
- Limits: it is a geometric offset, not a stroke-skeleton model, so very large thickening rounds counters shut and sharp inner corners stay mitred.

### Sliders modelled on the web tool (tools.oyasumi-gamedev.com/giongo)
- `Params` replaced `size_ramp` with **start_size / end_size** and gained **lean** (0 upright on the guide .. 1 bent along it; blend in `warp_letters`), **rise** (shear of horizontal strokes, degrees), **spacing_by_size** (letters are re-packed in `effects::packing_shifts`: gap x lerp(1, 1/avg size, k)), **keystone_v / keystone_h** (`geometry/keystone.rs`, runs after the guide warp, before the free warp).
- **Intensity macros** `power / roughness / momentum` (0..100, 50 neutral) apply only the *change* to the individual fields, with the web tool's measured coefficients: Power -> weight, start/end size; Roughness -> angle variation, size variation, offset jitter; Momentum -> lean, slant, rise.
- New built-in preset **Impact** = the site's default look. Old files with `size_ramp` still load (field ignored).
- Not copied: brush-only sliders (wobble, stretch, taper, dry brush, dakuten...), fills/tones/outline/shadow.

### Directional weight, no-overlap, rigid guide placement
- **Weight** = FontLab "Advanced": `weight` moves vertical stems, `weight + weight_contrast` horizontal strokes. `offset_xy` is an exact elliptical offset (scale y so the ellipse is a circle, i_overlay offset, scale back); `weight_round` switches mitre to round joins. Ramp still sits on top via `offset_path`.
- **`geometry/spacing.rs::separate_letters`** (param `no_overlap`, default on): after effects + weight, each letter is pushed along the flow axis (with the rest of its line) until the row-profile gap to the previous letter is >= min(baseline gap, 0.015em). Baseline = layout before effects, so deliberate tight/negative tracking survives. Only neighbours are checked.
- **Guide placement is rigid** by default (`warp::place_letters`): letters keep their shape (still curves), are moved to the guide, rotated by `lean` x tangent angle, and the whole text is scaled by guide length / text length. `bend_glyphs` restores the old per-point bending (`warp_letters`).

### Phase 3: English SFX Intelligence, Giongo Transform & FontLab 8 Simple Weight
- **English SFX Dictionary & Phonetic Auto-Pick** (`src/model/dictionary.rs`):
  - Pure English/Latin SFX catalog across 8 genres: Impact, Crash & Explosion, Speed & Slash, Rumble & Earthquake, Water & Splash, Cute & Sparkle, Small & Subtle, Shock & Surprise. Zero Japanese features.
  - Interactive chip gallery with one-click text assignment.
  - Rule-based phonetic text analyzer (`auto_pick_sfx`) classifying English consonant acoustics (plosives: B, P, D, T, K, G; sibilants: S, Z, SH, CH, W; vibrants: R, M, N), low vs high vowels, exclamation marks, and letter repetitions to automatically select sound presets and tune Power, Roughness, and Momentum.
  - Quick-action `✨ Auto Pick` button beside text input.
- **Giongo-Style Transform** (`src/geometry/free_warp.rs`, `src/app/tools.rs`):
  - Removed awkward Photoshop 8-handle selection box in favor of natural letter dragging.
  - Clean 3-mode Transform selector: `None` | `Trapezoid` | `Grid`.
  - **Trapezoid (台形)**: horizontal and vertical perspective skew (`trap_h`, `trap_v`).
  - **Grid (格子)**: 4x4 Bezier mesh warp with presets (Arc, Arch, Bulge, Flag, Rise, Squeeze), bend slider, `🎲 Roll warp` (procedural organic grid randomizer), and direct on-canvas point dragging.
- **FontLab 8 Simple (TransType/Fontographer) Weight Algorithm** (`src/geometry/weight.rs`):
  - Replicates FontLab 8's Simple (TransType/Fontographer) algorithm:
    - Primary weight adjustment with automatic proportional horizontal/vertical stem scaling (`weight_stem_ratio`, default 0.60 optical ratio).
    - **"Don't change vertical size"** (`weight_keep_v`): anchors vertical dimensions strictly to original cap-height and baseline.
    - **"Don't change horizontal size"** (`weight_keep_h`): preserves original glyph advance width.
    - **"Round corners"** (`weight_round`): selectable round joins vs sharp miters.

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
  app.rs                SfxApp: panels, input, selection drags, canvas texture caching, export UI
  app/widgets.rs       scroll-aware sliders
  app/library_ui.rs     font picker + library panel
  app/tools.rs          presets/roll/mutate UI, guide editing, brush, file ops, nudge
  fonts.rs              font folder scanning (no system fonts)
  settings.rs           persisted font folders
  library.rs            on-disk SFX library (folder per font)
  app/transform.rs      free transform handles + drag maths
  history.rs            generic undo/redo snapshots
  interact.rs           hit testing + group transform helpers (pure)
  export/               png.rs, svg.rs, bundle.rs (ZIP), project.rs (JSON)
  model/                pure data (serde-serializable), no UI/font handles
    dictionary.rs       English SFX dictionary + phonetic auto-pick engine
    letter.rs           Letter { ch, glyph_path (local space, y-down), transform }
    params.rs           Params (layout + effect sliders)
    scene.rs            Scene { text, direction, letters, seed }, Scene::bounds()
  geometry/             pure functions: paths in -> paths out (unit-testable, thread-safe)
    build.rs            layout -> effects -> warp (scene-build step)
    warp.rs             guide path sampling + glyph warp
    brush.rs            freehand stroke -> outline
    weight.rs           variable stroke weight (outline offset)
    free_warp.rs        4x4 Bezier warp grid
    effects.rs          seeded per-letter jitter / ramp / slant
    text_to_paths.rs    FontData, text_to_paths, EM_SIZE
  random/gacha.rs       roll / mutate params
  render/
    gpu.rs              OpenGL canvas renderer (tessellation + GL callback)
    raster.rs           render_scene(scene, view, w, h, background) -> Pixmap
assets/fonts/           bundled OFL fonts + LICENSES.md
```

Not implemented (skipped by request): M6 `geometry/outline.rs`, `render/tones.rs`, fills; M8 `geometry/liquify.rs`.

### Key conventions
- **Coordinates:** scene space is y-down. `EM_SIZE = 200` scene units. Glyph paths are stored at the origin; `Letter.transform` positions them. Font y-up is flipped in `PathSink`.
- **Two renderers:** the canvas uses the GPU path (`render/gpu.rs`); PNG export / Copy PNG use `render_scene` (tiny-skia). Both draw the same `Scene` letters with non-zero fill, so keep their geometry inputs identical.
- **View transform** in `app.rs`: `scale(ppp) * translate(pan) * scale(zoom)`. `pan` is in canvas-local points.
- **Re-render gating:** the GPU mesh is rebuilt when `(scene_version, zoom bucket)` changes. **Any code that mutates the scene must bump `scene_version`** (see `relayout`) or the canvas mesh won't update.
- **Errors:** no `unwrap()` on user paths. Errors go through `SfxApp::set_error` into the status bar.
- **Determinism:** seed lives in `Scene.seed`. Plan: per-letter RNG `ChaCha8Rng` seeded from `seed ^ letter_index` so results don't depend on thread order.

## How to modify

- **Add a geometry effect:** new fn in `geometry/` taking `&BezPath`/params and returning a new path; add unit tests; call it from the scene-build step (arrives in M4) rather than from UI code.
- **Add a UI control:** add the field to `model/`, edit it in `SfxApp::side_panel`, call `relayout()` (or bump `scene_version` if only visual).
- **Add a font:** drop an OFL file in `assets/fonts/`, add a row to `LICENSES.md`.
- **egui 0.37 API notes** (differs from older tutorials): `eframe::App` implements `ui(&mut self, ui, frame)` (not `update`); panels are `egui::Panel::left(..).show(ui, ..)`; `DroppedFile` is a trait (`path()`, `bytes()`).

## Known limitations / TODO
- Kerning uses the legacy `kern` table only (no GPOS); fonts with GPOS-only kerning get none.
- Font folders are scanned synchronously on startup / folder change (names only; bytes load on selection).
- File dialog is synchronous on the UI thread (fine for M1).
- Only the first face of a .ttc is used.
- `relayout()` (text/direction/font change) rebuilds letters, so manual transforms and selection reset; the change itself is undoable, but undo restores text/letters, not the font.
- Undo snapshots clone the whole scene (fine at this size; switch to transform-only deltas if scenes grow heavy).
- Scale handles are uniform only; no marquee selection yet.
- Warped letters are polylines (curves flattened), so SVG paths are larger. Brush strokes are not warped. In guide-edit mode clicking adds points instead of selecting.
- Changing text/sliders re-lays out text letters (manual transforms of text letters reset; brush strokes keep theirs).
- ZIP export is a fixed bundle (no multi-size batch).
