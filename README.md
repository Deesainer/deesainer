# Deesainer

An open-source interface design tool in Rust, for macOS, Windows, Linux and the browser: canvas, vector editing, text, styles and variables, auto layout, components, prototyping, live collaboration and export to PNG, JPEG, WebP, SVG and PDF. Every feature is also a command, available from the CLI, an MCP server and plugins.

**Status: public beta (1.1.0-beta.1).** Please [report problems](https://github.com/Deesainer/deesainer/issues).

## Download

Installers for macOS (universal), Windows and Linux are on the [releases page](https://github.com/Deesainer/deesainer/releases). The beta builds are not signed yet: on macOS right-click the app and choose Open the first time; on Windows choose More info, then Run anyway.

## Build from source

```sh
git clone https://github.com/Deesainer/deesainer && cd deesainer
cargo run --release -p deesainer -- --demo
```

## Licence

[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your choice. Assets: [ATTRIBUTION.md](ATTRIBUTION.md). Dependencies: [docs/third-party.md](docs/third-party.md).

Deesainer is an independent project, not affiliated with or endorsed by any other design tool vendor.
