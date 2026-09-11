# Design

The visual decisions, so they are not re-picked from scratch each time.

## The mark

A **thumbprint pressed into clay**. The app is something you shape by hand until
it fits you, and clay is already the governing metaphor through `PRINCIPLES.md` —
material that stays workable rather than setting hard. A print is the trace of a
hand on that material.

Source of truth is **`claya-icon.svg`** at the repo root — vector, 1024×1024.
Never hand-edit anything in `src-tauri/icons/`; that whole directory is generated:

```
npx tauri icon claya-icon.svg
```

That writes `icon.icns`, `icon.ico`, the PNG ladder, and the iOS/Android sets.
`tauri.conf.json` points at `icons/icon.png` and `icons/icon.icns`.

### Construction

- **Canvas** 1024×1024. Squircle 824×824 at (100,100), corner radius 186 — the
  standard macOS Big Sur proportions, so it sits correctly beside other Dock icons.
- **The rings are ovals, not circles**, and taller than they are wide. Real prints
  are oval; perfect circles read as a target.
- **The ring gaps are scattered to three different sides** — 138°, 302°, 38°. This
  is the one rule that matters. An earlier pass had the gaps roughly aligned and
  the result read unmistakably as a **wifi symbol**. Aligned gaps make a wedge, and
  a wedge is a signal icon. Scatter them.
- The arcs are computed, not hand-drawn — the generator that produced the path
  data is in the commit that introduced this file.

### Palette

| Role | Value | |
|---|---|---|
| Background | `#F0AC8B` → `#CE7551` → `#8E442A` | Three-stop terracotta, top-left to bottom-right |
| Print | `#FFF4E6` | Rings and core |
| Sheen | white `0.30` → `0.05` → `0` | Top-down gloss over the squircle |

Two things worth keeping:

- **Terracotta, not amber.** Fired-clay red-brown, deliberately warmer and earthier
  than orange. It should read as clay, not as juice or beer.
- **Cream, not white.** Pure `#FFFFFF` against saturated terracotta vibrates at
  small sizes; `#FFF4E6` sits still.

### Sizes

Reads cleanly to **32px**, and even at 16px stays a recognisable cluster of rings
rather than a blob. Check any icon change at 32px before judging it at 512px.

## Shell UI

The chat shell deliberately uses the **system canvas keywords** rather than fixed
hex — `Canvas`, `CanvasText`, and `color-mix(in srgb, CanvasText N%, transparent)`
for every border, fill and muted tone. That is what makes the drawer follow macOS
light/dark without a theme switch or a second palette to maintain.

So: no raw greys. A new surface is a `color-mix` against `CanvasText`.

The few fixed colours are the ones that carry meaning and must not drift with the
theme:

| Role | Value |
|---|---|
| Live / success | `#2a9d4a` |
| Failure, danger, rejected-by-the-gate | `#d33` |
| Key gate border | `#e8a` |

The shell palette is **not** the icon palette. Terracotta belongs to the mark and
to marketing surfaces; the app chrome stays neutral so model-authored canvases are
not fighting a brand colour for attention.

## Version list

Rows read left to right as a sentence: **`[Switch to] v3 — make background black`**.

The action sits in a fixed 76px column so the version numbers still line up
whether a row carries a button or the `live` badge — without the fixed width the
left edge goes ragged as soon as one row differs. `Reset to` lands in the same
column; check it still fits at 76px or widen it for every row at once.
