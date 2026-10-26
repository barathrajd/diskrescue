# DiskRescue

DiskRescue is a read-only recovery workspace for disk images. This repository
contains the initial Rust workspace, inspection CLI, and Tauri desktop shell.

> **Warning:** Work from a disk image on a separate healthy drive. Never repair,
> format, initialize, or mount the affected source drive read-write. If a drive
> clicks, disconnects, or returns severe I/O errors, stop and consult a
> professional recovery lab.

## Current status

The CLI can inspect MBR metadata and protective-MBR GPT headers from an image.
The desktop app is a UI preview: it selects an image file locally but does not
inspect, scan, or recover data yet. Overwritten sectors cannot be reconstructed.

## Start the app

Install the platform prerequisites and run commands from [the build guide](BUILD_TOOL.md).

```sh
pnpm install
pnpm desktop:dev
```

## Project layout

- `apps/desktop`: Tauri 2 and React desktop shell, built with Rstack.
- `apps/cli`: `diskrescue` command-line application.
- `crates/disk-reader`: bounded, read-only access to image files.
- `crates/partition-parser`: MBR and primary GPT parsing.
- `crates/recovery-core`: image inspection orchestration.
- Other crates are reserved for partition discovery, filesystem recovery,
  file carving, and safe export.

See [the implementation plan](DISKRESCUE_IMPLEMENTATION_PLAN.md) for the
recovery milestones and safety requirements.
