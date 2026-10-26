# Formatted HDD recovery

## Objective

Give the owner of a twice-formatted HDD a free, offline, best-effort way to
recover files using a Tauri desktop app on Windows, macOS, or Linux. The app
must work from a read-only disk image, which it can create on a separate healthy
drive or open if the user already created it. Recover usable file contents
first; preserve names and folders when filesystem metadata survives, and never
promise recovery of overwritten data.

## Success criteria

The first release is successful when these observable safety and recovery
checks pass.

- The same recovery workflow works from an existing image on Windows, macOS,
  and Linux.
- On platforms with a verified imaging helper, image creation confirms stable
  source details, opens the source read-only, writes to a separate destination,
  and stops safely on read errors or insufficient destination space. Where no
  helper is safely supported, direct imaging is disabled and existing-image
  recovery remains available.
- The scanner reports partition and filesystem candidates with offsets,
  confidence reasons, and unsupported or unreadable areas.
- Metadata-based recovery retains names and paths when supported metadata
  survives; carving reports generic names and does not imply original folder
  structure.
- The first supported catalog covers common document, photo, video, and archive
  formats, with per-file verified, partial, unsupported, or unrecoverable
  results.
- Export rejects unsafe paths, never writes to the source, and does not replace
  existing files without explicit consent.
- The complete workflow runs without network access and does not require a paid
  service or license.

## In scope / Out of scope

This plan targets one recovery attempt on the user's formatted HDD while
establishing a reusable desktop recovery workflow.

**In scope**

- Tauri desktop support for Windows, macOS, and Linux, using the existing Rust
  workspace and Rstack frontend.
- Selecting an existing raw image or creating an image from a physical HDD onto
  a separate healthy destination.
- Read-only image inspection, lost partition and filesystem discovery, deleted
  file metadata recovery where supported, and signature-based carving for a
  defined catalog of common file formats.
- Previewing and selecting candidates, then exporting to a separate destination
  with safe path and conflict handling.
- Clear warnings about formatting, overwrite limits, partial files, and drive
  failure symptoms.

**Out of scope**

- Repairing, formatting, initializing, mounting read-write, or otherwise writing
  to source media.
- Guaranteed recovery, reconstruction of overwritten sectors, or a promise to
  support every file format or filesystem in the first release.
- Recovery from SSDs, phones, RAID arrays, encrypted filesystems without keys,
  or failing drives that need hardware-level lab work.
- Network upload, cloud recovery, paid recovery services, and recovery output on
  the source drive.

## Approach

Keep parsing and recovery in Rust crates; keep `apps/desktop` as the Tauri
presentation layer. Route both physical-device imaging and existing-image input
into the same read-only image pipeline, and require a separate output location
for both the image and recovered files. Use filesystem metadata first because
it can retain names and folders, then use validated signatures for supported
formats when metadata is missing. Signature-based recovery commonly loses
original names and folder structure, as [PhotoRec's documentation explains](https://www.cgsecurity.org/testdisk_doc/photorec.html).

The repo already has a bounded read-only image reader and basic MBR/GPT image
inspection, while the file-carving crate is a placeholder. The existing
implementation plan names NTFS and ext4 as initial metadata targets and JPEG,
PNG, PDF, and ZIP as initial carving formats; this plan extends the carving
catalog to common videos and documents through fixture-backed format work.
Microsoft documents quick format as removing the file table and root directory,
and documents sector zeroing separately under `/P`; the user's recollection of
“full format” alone does not establish what was overwritten. The app must make
no recovery prediction until it inspects an image. See [Microsoft's format
reference](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/format).

Tauri documents Windows, macOS, and Linux development prerequisites, but
physical-device enumeration and access require platform-specific privilege
handling. Keep the app unprivileged and isolate imaging in narrow helpers.
Microsoft recommends a separate helper for elevated Windows operations;
macOS's current `SMAppService` path starts at macOS 13, and Linux authorization
depends on polkit and the desktop session. If a safe helper is unavailable, keep
existing-image recovery and guide the user to a platform-supported imaging
tool. See [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/),
[Windows privilege guidance](https://learn.microsoft.com/en-us/windows/win32/secbp/running-with-administrator-privileges),
[Apple Service Management](https://developer.apple.com/documentation/servicemanagement/smappservice),
and [polkit](https://www.freedesktop.org/software/polkit/docs/latest/polkit.8).
The [GNU ddrescue manual](https://www.gnu.org/software/ddrescue/manual/ddrescue_manual.html)
documents mapfile-based imaging for difficult reads.

## Milestones

Build each milestone as a usable slice from image input through visible results
and safe export.

1. [x] **Open an existing image.** Select an image through a native file picker,
   inspect its MBR/GPT metadata in Rust, and show the image size and findings.
2. [x] **Write an image safely.** Copy a bounded read-only source in chunks to a
   temporary file in the destination filesystem. Check free space first, report
   progress, cancel without publishing a partial image, and refuse overwrite.
3. [x] **List physical sources safely.** Show available identity and mount details;
   disable imaging when stable identity or required access is unavailable.
4. **Image a physical HDD.** Use a platform-specific privileged helper to
   confirm model, serial, and capacity, then revalidate the source and separate
   destination before imaging. Reopen the completed image in the inspection
   flow.
   - [x] Add a bounded reader that opens physical devices with read access only.
   - [ ] Add platform-specific helper authorization and source revalidation.
   - [ ] Confirm the destination is on a different physical device and reopen
     the completed image in the inspection flow.
5. **Find the former volume.** Scan the image for partition and filesystem
   candidates, including supported backup metadata and damaged structures.
   Show offsets, confidence reasons, and unsupported areas before recovery.
6. **Recover metadata-backed files.** For the first supported filesystems,
   list recoverable deleted entries with names and paths where metadata
   survives; let the user export selected files to a separate destination.
7. **Carve common file formats.** Add bounded scanners and format validators
   for the agreed common document, photo, video, and archive catalog. Show
   generic names, file integrity status, and likely partial results.
8. **Finish the desktop recovery session.** Add filtering, preview, progress,
   cancellation and resumable checkpoints; record source-image provenance and
   export results without overwriting files silently.
9. **Verify supported platforms and safety.** Exercise image intake, scanning,
   cancellation, and export on Windows, macOS, and Linux with synthetic images;
   document platform gaps and ensure unsupported paths fail closed.

## Risks & open questions

Resolve these items during implementation or use the listed fallback.

- **Physical imaging permissions and device identification differ by OS.**
  Owner: implementation. Keep the app unprivileged and run a read-only
  acquisition spike for each helper. Windows needs UAC elevation, macOS helper
  registration requires supported signing and macOS 13 or later for
  `SMAppService`, and Linux needs an available polkit authentication agent.
  Fallback to existing-image input plus documented external imaging wherever a
  helper fails safety checks or cannot be authorized.
- **The source filesystem and exact format operation are unknown.** Owner:
  recovery engine. Inspect partition and filesystem signatures in the image;
  initially support NTFS and ext4 metadata paths from the existing
  implementation plan, and use supported carving when metadata recovery does
  not apply.
- **“Common formats” needs a fixture-backed first-release catalog.** Owner:
  product and implementation. Finalize the catalog before carving work; fallback
  is to ship only validators with synthetic fixtures and label other types
  unsupported.
- **A readable preview does not prove a complete file can be recovered.** Owner:
  recovery engine. Validate exported file structure where possible and report
  partial or corrupt results without promising success.
- **Repeated formatting may have overwritten metadata or file contents.**
  Owner: user-facing workflow. Preserve the source, scan only an image, and
  report findings without estimating recovery odds from the word “full.”

## Boundaries

These project safety rules apply to every milestone and supported platform.

- Treat the physical source as hostile, read-only input; never expose a write,
  repair, format, initialize, or source-mount operation.
- Store the disk image and recovered output on separate healthy destinations;
  reject unsafe paths and require explicit consent before replacing a file.
- Keep image data and recovery results local and offline.
- Use checked arithmetic, bounded reads and allocations, and graceful errors for
  corrupt or truncated image content.
- State that overwritten sectors cannot be reconstructed and distinguish
  verified, partial, unsupported, and unknown results.
