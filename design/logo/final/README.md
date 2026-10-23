# Omarchy Sync logo

The "o" from the Omarchy wordmark (`/usr/share/omarchy/logo.svg`), cut across the middle with its halves slid one pixel-step apart: two copies lining up.

## Files

- `omarchy-sync-symbol.svg`: master, black. The other symbol files are made from it.
- `-white`, `-black`, `-mono-94e2d5`: one-colour versions.
- `-app-icon`: teal mark on a dark tile. `src-tauri/icons/icon.png` is its 512 px render.
- `-square`, `-favicon`, `favicon.*`, `icon-*.png`, `site.webmanifest`: avatar and web icons.
- `omarchy-sync-horizontal-dark` / `-light`: symbol with the name. The name is live text in JetBrains Mono (Omarchy's default font), so use the PNGs, or convert the text to outlines before you send the SVG anywhere.

## Colour

| Use | Hex |
|---|---|
| Mark | `#94e2d5` (the Sync tile colour in the app) |
| Tile / dark background | `#1e1e2e` |
| Name on dark | `#cdd6f4` |

Use one colour for single-colour printing: black, white or teal.

## Rules

- Keep clear space of one stroke width (the width of one side of the o) around the mark.
- Minimum size: 16 px. Below about 24 px the gap closes and it reads as a plain o, which is fine.
- Don't round the stepped corners, add outlines or shadows, stretch the mark, or close the gap.
- This is an unofficial app. The design borrows Omarchy's letter style; say "unofficial" wherever the logo appears publicly.

## Regenerating

```bash
python3 ~/.claude/skills/logo-design/scripts/export_variants.py omarchy-sync-symbol.svg \
  --title "Omarchy Sync" --mono "#94e2d5" --icon-bg "#1e1e2e" --icon-fg "#94e2d5" --png 512 1024 --web-icons
cp omarchy-sync-symbol-app-icon-512.png ../../../src-tauri/icons/icon.png
```
