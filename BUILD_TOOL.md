# Build tool

DiskRescue uses Rstack for the desktop frontend and Cargo for the Rust
workspace. Tauri packages the Rsbuild output into a native desktop app.

## Requirements

Install Node.js 24.3.0 or newer, pnpm 12.15.0, and the Rust toolchain. Tauri
also requires the platform-specific system libraries listed in the
[Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/).

## Install dependencies

Run the following command from the repository root:

```sh
pnpm install
```

Cargo resolves Rust dependencies when you build the workspace.

## Run the desktop app

Start the Tauri development app from the repository root:

```sh
pnpm desktop:dev
```

Build the desktop app with:

```sh
pnpm desktop:build
```

Build the web shell without opening Tauri with:

```sh
pnpm desktop:web
```

## Inspect an image with the CLI

The initial CLI reads a disk image and reports MBR/GPT metadata. It opens the
image read-only and does not repair or write to it.

```sh
cargo run -p diskrescue-cli -- inspect /path/to/disk.img
cargo run -p diskrescue-cli -- inspect /path/to/disk.img --json
```

The desktop image selection screen is a preview. Its partition inspection and
recovery actions are not connected yet.
