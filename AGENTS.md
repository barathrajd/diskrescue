# Project instructions

DiskRescue handles potentially failing media and sensitive recovery data. Keep
all source-media access read-only. Build recovery features around disk images;
never add repair, format, initialize, or source-write operations.

The Rust workspace owns parsing and recovery logic. `apps/desktop` is the Tauri
presentation layer, and `apps/cli` is the reproducible command-line interface.
Keep parsing bounded, check integer arithmetic, and treat image contents as
hostile input. Do not promise recovery of overwritten data.

Recovered output must go to a separate destination. Reject unsafe paths and do
not overwrite existing files without explicit consent. Keep recovery data
offline and out of source control.

Use Rstack (`rs`) for the desktop frontend and Cargo for Rust code. Read
`DISKRESCUE_IMPLEMENTATION_PLAN.md` before expanding recovery scope.
