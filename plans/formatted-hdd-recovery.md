# Formatted HDD recovery

## Objective

Give the owner of a formatted HDD a free, offline, best-effort way to find lost
partitions and recover files using a Tauri desktop app on Windows, macOS, or
Linux. Prefer a read-only disk image on a separate healthy drive. Where a
platform supports a narrowly authorized read-only helper, also let the user
select an external drive and scan it directly without a file picker. Recover
usable file contents; preserve names and folders when filesystem metadata
survives, and never promise recovery of overwritten data.

## Success criteria

The first release is successful when these observable safety and recovery
checks pass.

- The Rust recovery engine opens and scans existing images on Windows, macOS,
  and Linux. The desktop starts with physical-drive selection when safe direct
  scanning is supported.
- The desktop can scan a confirmed external physical drive without relying on
  its current partition table, but only through a platform-authorized helper
  that opens the source read-only and revalidates its identity before each scan.
  If authorization is unavailable, the app explains the blocker and leaves the
  source untouched.
- Whole-drive scans report validated lost-partition candidates with filesystem
  type, offset, estimated length, confidence reasons, and overlaps. The app
  does not write a new partition table or imply that every signature marks a
  recoverable volume.
- Filesystem support is selected from validated on-disk structures, not only
  the partition type label reported by the operating system. NTFS and ext4 are
  the first metadata targets; unsupported or ambiguous filesystems are labeled
  clearly.
- On platforms with a verified imaging helper, image creation confirms stable
  source details, opens the source read-only, writes to a separate destination,
  and stops safely on read errors or insufficient destination space. Where no
  helper is safely supported, direct imaging is disabled and existing-image
  recovery remains available.
- The scanner reports partition and filesystem candidates with offsets,
  confidence reasons, and unsupported or unreadable areas.
- Scans report their current phase and byte progress, can be cancelled safely,
  and can resume from a durable checkpoint only when the source-image identity
  and scan format still match.
- Metadata-based recovery retains names and paths when supported metadata
  survives; carving reports generic names and does not imply original folder
  structure.
- The first supported catalog focuses on common photo and video formats, with
  per-file verified, partial, unsupported, or unrecoverable results.
- Actual file results can be browsed by both type and original path, searched,
  filtered, selected in batches, and previewed when a bounded supported preview
  is available.
- Local scan records can be reopened offline; completed results can be viewed,
  but previews and exports require source identity validation. Incomplete
  records resume only after that validation.
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
- Read-only image inspection and, where a safe helper is available, direct
  read-only scanning of confirmed external physical drives. The desktop starts
  with drive and partition selection; it does not require choosing an image
  through a Finder or file-picker dialog.
- Lost-partition and filesystem discovery across the whole selected disk,
  including regions not described by the current partition table.
- Deleted-file metadata recovery for supported NTFS and ext4 volumes, followed
  by signature-based carving for a fixture-backed catalog of common photos and
  videos.
- Previewing and selecting candidates, then exporting to a separate destination
  with safe path and conflict handling.
- A DiskRescue-designed results workspace with selectable **Path** and **Type**
  views, common file categories, search, filters, multi-selection, and bounded
  previews for supported images and videos.
- Offline scan records that reopen completed results or resume a compatible
  incomplete checkpoint.
- Clear warnings about formatting, overwrite limits, partial files, and drive
  failure symptoms.

**Out of scope**

- Repairing, formatting, initializing, mounting read-write, or otherwise writing
  to source media.
- Guaranteed recovery, reconstruction of overwritten sectors, or a promise to
  support every file format or filesystem in the first release.
- Cloud recovery, video repair of already recovered files, bootable installer
  creation, and other separate EaseUS utilities that do not advance this
  formatted-HDD workflow.
- Copying EaseUS screens, branding, or proprietary implementation details.
- Recovery from SSDs, phones, RAID arrays, encrypted filesystems without keys,
  or failing drives that need hardware-level lab work.
- Network upload, cloud recovery, paid recovery services, and recovery output on
  the source drive.

## Approach

Keep parsing and recovery in Rust crates; keep `apps/desktop` as the Tauri
presentation layer. Give image files and authorized physical sources the same
bounded `ReadAt` scanning interface. Image creation writes only to a separate
healthy destination. Direct physical scans use a narrow platform helper that
revalidates the external source, requires all source volumes to be unmounted,
and opens the device read-only. Never run the Tauri UI as root or change device
permissions to bypass authorization.

Scan the whole disk for validated partition and filesystem structures before
presenting candidate partitions. Use filesystem metadata first because it can
retain names and folders, then use validated signatures for supported formats
when metadata is missing. Signature-based recovery commonly loses original
names and folder structure, as [PhotoRec's documentation
explains](https://www.cgsecurity.org/testdisk_doc/photorec.html).

Use REA's inspection of the installed EaseUS 20.9.0 bundle and the user's
screenshots as functional references only. They show drive and lost-partition
selection, phase/progress controls, Path/Type browsing, categories, previews,
saved scan records, and separate recovery destinations. The bundle also
declares `.rsf` scan-result and `.edi` disk-image associations. Interface
resource names indicate additional cloud, unbootable-Mac, SD-card, video-repair,
backup, and installer screens; those names do not prove runtime behavior. REA
identified the app and plist, and decoded a bounded subset of interface
resources. Their Evidence IDs are:

- App inventory:
  `ev_fa90971f1ab6ceb6cb5bbccf16bdfef50517f7312e321234d11f976700066425`
- Bundle plist:
  `ev_215c0db78c58e791f92da089cc0c1817002b6908cfd5180290b26991823bb636`
- Interface resources:
  `ev_6f5693ba9bdc9879deb34c52982216c25d9639e1f64eecff9fbe61a955a26b6`

REA's decoder capped that pass at 64 documents, so treat unobserved screens as
unknown. Keep DiskRescue's current visual language rather than recreating
EaseUS's layouts.

Keep scan history and its paged result index in a Rust-owned local SQLite
catalog under Tauri's app-specific data directory. Store a source-image
fingerprint, scan options/version, status, progress checkpoints, and
recovered-file metadata; do not copy source file contents or thumbnails into
the catalog. Compute a full image digest while creating an image or during its
first file-level scan, then recheck the digest before resuming, previewing, or
exporting a saved session.
Tauri's `PathResolver::app_data_dir` provides the application data location,
and the existing Rust command/event boundary can carry small progress updates
while commands page file results. See [Tauri's app data
path](https://docs.rs/tauri/latest/tauri/path/struct.PathResolver.html),
[Tauri's event guidance](https://v2.tauri.app/develop/calling-frontend/), and
[`rusqlite`](https://docs.rs/rusqlite/latest/rusqlite/) for the Rust SQLite
interface. Persist checkpoint state only after a complete bounded scan unit;
if the source digest or scanner version cannot be verified, mark the record
stale and offer a new scan from the start.

The repo already has a bounded read-only image reader and basic MBR/GPT image
inspection, while the file-carving crate is a placeholder. The existing
implementation plan names NTFS and ext4 as initial metadata targets and JPEG,
PNG, PDF, and ZIP as initial carving formats; this plan extends the carving
catalog to common videos and documents through fixture-backed format work.
Microsoft documents quick format as removing the file table and root directory,
and documents sector zeroing separately under `/P`; the user's recollection of
“full format” alone does not establish what was overwritten. The app must
make no recovery prediction until it inspects an image. See [Microsoft's format
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
The [GNU ddrescue manual]
(https://www.gnu.org/software/ddrescue/manual/ddrescue_manual.html) documents
mapfile-based imaging for difficult reads.

## Milestones

Build each milestone as a usable slice from a disk image or authorized physical
source through visible results and safe export.

1. [x] **Open an existing image.** Select an image through a native file picker,
   inspect its MBR/GPT metadata in Rust, and show the image size and findings.
2. [x] **Write an image safely.** Copy a bounded read-only source in chunks to a
   temporary file in the destination filesystem. Check free space first, report
   progress, cancel without publishing a partial image, and refuse overwrite.
3. [x] **List external sources safely.** List only drives confirmed as external
   from native device metadata; hide internal and unknown devices, load the
   external list on startup, and disable imaging when stable identity or
   required access is unavailable.
4. **Authorize physical-device access safely.** Use a platform-specific helper
   for imaging and direct scans. Revalidate device identity, external status,
   system-disk status, mount state, capacity, and requested byte range before
   every operation. Keep the UI unprivileged.
   - [x] Add a bounded reader that opens physical devices with read access only.
   - [x] Add a fail-closed guard for identity revalidation, mounted/system
     sources, and source/destination device separation.
   - [ ] Add platform-specific helper authorization for imaging and scanning;
     keep macOS direct access disabled until its helper is safely authorized.
   - [ ] Confirm the destination is on a different physical device and reopen
     the completed image in the inspection flow.
5. [ ] **Find the former volume.** Scan images and authorized physical sources
   for filesystem candidates, including supported backup metadata and damaged
   structures. Show offsets, confidence reasons, and unsupported areas before
   recovery.
   - [x] Inspect MBR/GPT partition entries and check partition starts for
     structurally consistent NTFS or ext4 signatures.
   - [x] Recover partition entries from a valid backup GPT when the primary GPT
     header or table is damaged.
   - [x] Search image regions at 512-byte boundaries for validated NTFS and
     ext4 starts, skipping known partitions and supporting progress and
     cancellation.
   - [ ] Generalize lost-filesystem discovery over `ReadAt` so it can scan an
     authorized physical drive as well as an image.
   - [ ] Scan the complete selected disk in bounded chunks, independent of the
     current partition table; report candidate offsets, lengths, confidence,
     and overlaps with progress and cancellation.
   - [ ] Show discovered lost-partition candidates in the partition-selection
     page and let the user open a candidate's separate recovery workspace.
6. **Recover metadata-backed files.** For the first supported filesystems,
   list recoverable deleted entries with names and paths where metadata
   survives; let the user export selected files to a separate destination.
   - [x] Parse NTFS boot-sector geometry and validate the partition and MFT
     bounds.
   - [x] Validate MFT record update-sequence fixups and parse `$FILE_NAME`
     metadata with parent references.
   - [x] Decode bounded, signed NTFS data runlists for nonresident stream
     extents.
   - [x] Parse bounded MFT record windows and list deleted records while
     ignoring unused slots.
   - [x] Parse the unnamed nonresident `$DATA` runlist and size from an MFT
     base record.
   - [x] Map fragmented MFT extents to bounded image reads and list deleted
     entries end to end, rejecting extents outside the volume.
   - [x] Expose the read-only NTFS deleted-entry scan through `recovery-core`.
   - [x] Wire selected NTFS volume scans into the desktop and show deleted
     names and sizes in the photo/video/all-files browser, marked as metadata
     only until content recovery is available.
   - [ ] Detect actual filesystem structures instead of trusting OS partition
     type labels such as “Linux Filesystem.”
   - [ ] Implement bounded ext4 deleted-entry scanning and file-extent
     validation for the user's reported Linux-filesystem partitions.
   - [ ] Resolve surviving parent paths and recover supported data streams to
     a separate destination.
7. **Carve common photos and videos.** Add bounded signature scanners and
   format validators for a fixture-backed first-release catalog, starting with
   JPEG, PNG, and common MP4/MOV containers. Show generic names, file integrity
   status, and likely partial results; do not promise original names or paths.
8. **Add resumable scan sessions.** Deliver real scan state from the Rust
   scanner to the desktop and persist it for offline review.
   - [x] Put external-drive selection before scanning; hide internal and
     unknown drives and omit any source-mount action.
   - [x] Load partition identifiers, names, sizes, filesystem labels, and
     mount state from OS device metadata when an external drive is selected.
   - [x] Separate drive selection, partition selection, and partition
     recovery into navigable desktop pages with a back path.
   - [x] Add a photo/video results workspace with type/path navigation and a
     truthful empty state until file-level recovery data is available.
   - [ ] Make whole-disk scanning the physical-drive path: show existing and
     discovered partitions, then navigate to a separate scan-results page.
   - [ ] Explain authorization, unsupported filesystem, and scan errors using
     the underlying reason; never show a generic failure for a known blocker.
   - [ ] Show scan phase, bytes checked, total bytes, and volume/file counts;
     cancel at a safe boundary and persist a checkpoint.
   - [ ] Save scan records offline with source fingerprint, scan version,
     status, timestamps, progress, and result counts; reopen completed records
     and resume an incomplete scan only after digest validation.
9. **Browse and export actual recovered files.** Populate results from
   metadata recovery and carving, then export selected candidates safely.
   - [ ] Provide **Path** and **Type** views, common categories, search,
     filters, result counts, and batch selection from actual results only.
   - [ ] Select and verify a destination on a different physical device, check
     available space, resolve conflicts before writing, and report per-file
     export status.
10. **Preview supported media.** Generate bounded previews for validated
    image/video candidates and explain unsupported or partial previews.
    - [ ] Keep preview reads bounded and never treat a preview as proof that
      the complete file can be recovered.
11. **Verify supported platforms and safety.** Exercise image intake, scanning,
    cancellation, and export on Windows, macOS, and Linux with synthetic
    images; document platform gaps and ensure unsupported paths fail closed.

## Next work session

The next implementation session targets the gap exposed by the first physical
drive scan: DiskRescue cannot yet read macOS raw devices as an unprivileged app,
and the selected partition is labeled “Linux Filesystem” while the connected
metadata scanner only supports NTFS. The macOS offset lookup and scan-error
reporting are now fixed. Continue in this order:

1. Design and implement a narrow read-only macOS helper authorization path;
   do not run the desktop app as root or relax `/dev` permissions. Keep physical
   scanning disabled if identity or authorization cannot be verified.
2. Generalize the existing lost-filesystem scan to the physical reader, then
   show whole-disk lost-partition candidates in the drive workflow.
3. Add ext4 metadata scanning and photo/video carving before presenting the
   workflow as file recovery. Export recovered content only to a separate
   destination.

The first usable gate is a synthetic whole-disk fixture that produces a
validated lost-partition candidate with the expected offset, length, and
filesystem type. Do not run recovery against the original drive until the
helper, bounds checks, and read-only behavior are verified.

## Risks & open questions

Resolve these items during implementation or use the listed fallback.

- **Physical-device access permissions and device identification differ by OS.**
  Owner: implementation. Keep the app unprivileged and use a narrow read-only
  helper. Windows needs UAC elevation, macOS helper registration requires
  supported signing, and Linux needs an available polkit authentication agent.
  Fallback to image-based scanning wherever a helper fails safety checks or
  cannot be authorized.
- **macOS helper signing credentials are not currently available.** Owner:
  release. Keep direct physical scanning and imaging disabled in release builds
  until DiskRescue can ship and validate a properly authorized helper. Do not
  bypass this by changing device permissions or running the whole UI as root.
- **The source filesystem and exact format operation are unknown.** Owner:
  recovery engine. Validate on-disk filesystem structures instead of relying on
  partition labels. Initially support NTFS and ext4 metadata paths, then use
  supported photo/video carving when metadata recovery does not apply.
- **“Common formats” needs a fixture-backed first-release catalog.** Owner:
  product and implementation. Finalize the catalog before carving work; fallback
  is to ship only validators with synthetic fixtures and label other types
  unsupported.
- **A readable preview does not prove a complete file can be recovered.** Owner:
  recovery engine. Validate exported file structure where possible and report
  partial or corrupt results without promising success.
- **A saved checkpoint may refer to a moved or changed image.** Owner: recovery
  engine. Bind each record to a source digest and scanner version; when the
  digest cannot be revalidated, mark the record stale, disable preview and
  export, and require a new scan instead of resuming.
- **Full-image identity checks add read time for existing images and resumes.**
  Owner: recovery engine. Compute the digest during imaging or the first
  file-level scan; if digest validation cannot finish, keep the source
  untouched and require a fresh scan before preview or export.
- **Large scans can produce a result catalog too large for an in-memory UI.**
  Owner: recovery engine. Page rows from the local SQLite catalog and keep
  previews outside the catalog; if catalog performance is insufficient, narrow
  the first-release result limit and expose the truncation clearly.
- **The common-format list is not yet extension-specific.** Owner: product and
  recovery engine. Finalize the fixture-backed list before carving; keep each
  unsupported type visible as unsupported rather than implying recovery.
- **Preview decoders receive hostile file contents.** Owner: recovery engine.
  Use bounded, format-aware decoders and show preview unavailable when a format
  cannot be validated safely.
- **Repeated formatting may have overwritten metadata or file contents.**
  Owner: user-facing workflow. Preserve the source, scan only an image, and
  report findings without estimating recovery odds from the word “full.”

## Boundaries

These project safety rules apply to every milestone and supported platform.

- Treat the physical source as hostile, read-only input; never expose a write,
  repair, format, initialize, or source-mount operation.
- Store the disk image and recovered output on separate healthy destinations;
  reject unsafe paths and require explicit consent before replacing a file.
- Treat the app-data scan catalog as local session metadata, not as a recovery
  destination; never place it on or write it through the selected source.
- Keep image data and recovery results local and offline.
- Use checked arithmetic, bounded reads and allocations, and graceful errors for
  corrupt or truncated image content.
- State that overwritten sectors cannot be reconstructed and distinguish
  verified, partial, unsupported, and unknown results.
