# Changelog

Every published release, newest first. Each entry is also the release notes
of the matching tag (`docs/releases/<version>.md`).

## 1.1.0-beta.4 — 2026-10-14

- **Colour management on the Mac**: on wide-gamut displays the canvas is
  presented in Display P3 — the GPU blit and the CPU and mesh paths convert
  from sRGB at presentation time, detected from the display profile's
  primaries. Exports stay sRGB; Windows and Linux are unchanged.
- **Windows**: the app no longer opens a console window next to its own
  (GUI subsystem in release builds), and the relay it hosts stays windowless.
- **Figma import**: the GLASS effect maps to a background blur, and SVG and
  Figma import are offered from the file name menu.
- **Tools**: the parity check tolerates CRLF checkouts.

## 1.1.0-beta.3 — 2026-10-13

More of the interface translated: the design panel rows (appearance, opacity,
corner radius, layout modes, auto width/height, fixed size, size limits and
the paint kind names). The translation tables hold 231 strings per language,
guarded by a regression test.

## 1.1.0-beta.2 — 2026-10-13

- **Interface languages**: the whole chrome — menus, palette, sidebars, the
  layers and pages panels, the design panel, the Inspect tab, components and
  auto layout, assets, comments, version history and design checks — in
  English, Italian, Spanish, French, German, Simplified Chinese and Japanese,
  switchable live from View ▸ Language and remembered across starts.
  Chinese and Japanese load a CJK-capable system font. The engine, the
  commands and the docs stay in English.
- **MCP edits the open app**: `deesainer-cli mcp` bridges into a running
  Deesainer automatically (the app publishes its control server in
  `~/.deesainer/control.json`), so an agent's commands appear on the canvas
  as they happen; `--standalone` keeps the headless document. The server
  tells the agent which document the app has open.
- **Windows installer**: an NSIS installer (`Deesainer-…-windows-x64.exe`)
  with an uninstaller and file associations, alongside the portable zip.
- **Linux package**: the `.deb` declares its runtime dependencies and the
  release pipeline smoke-tests the installed package headlessly (CLI, GUI
  over the control protocol, relay, team server).
- **Fixes**: the Windows plugin tests no longer crash (wasmi built
  unoptimised in dev builds); the licence table is platform-stable; the
  checksums file no longer contains its own hash; the Windows executables
  embed the app icon.

## 1.1.0-beta.1 — 2026-10-12

The first public release: the document model with undo, the CPU renderer and
GPU compositing, text shaping, booleans and vector networks, auto layout,
components with variants and slots, styles and variables, the native format
with version history, PNG/JPEG/WebP/SVG/PDF export, multiplayer with
branches and visual review, prototyping with smart animate, the MCP server,
the plugin sandbox, and a preview of the team server. See
`docs/releases/1.1.0-beta.1.md`.
