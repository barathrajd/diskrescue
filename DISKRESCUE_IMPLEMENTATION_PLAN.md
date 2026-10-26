# DiskRescue — Recovery Software Implementation Plan

## Project identity
- **Product:** DiskRescue
- **Tagline:** Recover Lost Files & Partitions
- **CLI executable:** `diskrescue`
- **Core engine:** Rust (`recovery-core`)
- **Desktop:** Tauri 2 + React 19 + TypeScript
- **Repository:** `diskrescue/`
- **Naming note:** DiskRescue is a working name; check existing products, trademarks, domains, and repository names before public release.

## Scope and incident
- Source: detected HDD; Windows/Linux dual boot; OS reinstalled; partitions and files now missing.
- First objective: preserve source, image it, then recover from the image.
- No guarantee of recovery: overwritten sectors cannot be reconstructed.

## Non-negotiable safety
1. Stop booting from or writing to the affected HDD.
2. Never repair, initialize, format, or mount the source read-write.
3. Use a Linux live USB and an independent healthy destination drive.
4. Confirm drive identity using model, serial and capacity before imaging.
5. If mechanical symptoms, disconnects or severe I/O errors occur, prioritize a professional lab.
6. Treat all disk images and recovered data as sensitive, and work offline.

## Safe recovery workflow

Prefer imaging the source to a separate healthy destination, then scan the
image. The desktop also plans a direct-scan path for confirmed external drives;
that path stays disabled until a narrow platform helper can authorize and
revalidate read-only access.

1. Boot a Linux live USB with automount disabled if possible.
2. Identify drives: `lsblk -o NAME,SIZE,MODEL,SERIAL,FSTYPE,MOUNTPOINTS` and `ls -l /dev/disk/by-id/`.
3. Mount only the healthy destination disk and verify free capacity >= source HDD capacity for a raw image.
4. Install GNU ddrescue (`sudo apt install gddrescue` on Ubuntu).
5. After verifying identifiers, run:
   `sudo ddrescue -n /dev/disk/by-id/SOURCE_DISK /mnt/healthy-drive/diskrescue.img /mnt/healthy-drive/diskrescue.map`
6. Keep the mapfile. Do not retry aggressively if the drive is failing.
7. Scan the image when available and recover files to a different physical disk.
   A direct physical scan must use read-only access, keep every source volume
   unmounted, and write nothing to the source.

## Proposed repository layout
```text
diskrescue/
├── apps/
│   ├── cli/
│   └── desktop/
├── crates/
│   ├── recovery-core/
│   ├── disk-reader/
│   ├── partition-parser/
│   ├── partition-hunter/
│   ├── fs-ntfs/
│   ├── fs-ext4/
│   ├── file-carver/
│   └── recovery-exporter/
├── docs/
├── fixtures/
├── BUILD_TOOL.md
└── Cargo.toml
```

## Architecture
- `apps/desktop`: Tauri 2, React 19, TypeScript, shadcn/ui, Zustand.
- `crates/disk-reader`: read-only random-access trait, file-backed image reader, bounded reads, overflow checks.
- `crates/partition-parser`: protective MBR, GPT headers and CRC checks, primary/backup GPT, candidate boundaries.
- `crates/partition-hunter`: NTFS boot sector and ext4 superblock candidates
  from images or an authorized read-only physical source; confidence scoring.
- `crates/fs-ntfs`: MFT records, fixup arrays, runlists, deleted records, directories and export.
- `crates/fs-ext4`: superblocks, group descriptors, inodes, extents, journal analysis where feasible.
- `crates/file-carver`: JPEG, PNG, PDF, ZIP, file boundary detection and validation.
- `crates/recovery-core`: scan orchestration, cancellation, checkpoints, progress and provenance.
- `crates/recovery-exporter`: path sanitization, conflict handling, streaming export to other disk.
- `apps/cli`: reproducible CLI to validate the engine before GUI integration.
- `docs`: Rspress documentation.

## Phased milestones
### M0: Incident preservation (immediate)
- Image affected HDD with ddrescue or seek professional help if failing.
- Record capacity, model, and SMART indicators when safe, along with the
  imaging map and hash of the captured image.
- Until direct-scan helper authorization and safety checks are complete, do not
  run the new tool on the original source.

### M1: Read-only engine and test harness (1–2 weeks)
- Implement `ReadAt` trait and image-backed reader.
- Strict read-only open; no write API in disk reader.
- Parse MBR/GPT including backup GPT and corruption warnings.
- CLI `diskrescue inspect image.img --json`.
- Synthetic images and snapshot-based tests.

### M2: Lost partition discovery (2–3 weeks)
- Scan at bounded offsets for filesystem signatures.
- Validate NTFS boot sectors and ext4 superblocks using multiple structural checks.
- Distinguish active vs historical partition candidates.
- Show confidence, offsets, length estimates, and reasons; do not modify partition tables.
- Run the same bounded discovery against an authorized external physical source,
  scanning the whole disk rather than only the partitions in its current table.
- Report progress and support safe cancellation for both image and physical
  scans.

### M3: NTFS metadata recovery (3–6 weeks)
- Parse MFT and deleted records; map nonresident data runs.
- Recover names and directories when metadata survives.
- Detect sparse, compressed, encrypted, and overwritten extents; clearly flag unsupported cases.

### M4: ext4 metadata recovery (4–8 weeks)
- Read superblock, group descriptors, inode tables, extents.
- Identify candidate deleted inodes and directory entries.
- Explain limits due to reuse, journal activity, and overwritten blocks.

### M5: Raw file carving (2–4 weeks)
- Implement JPEG, PNG, PDF, ZIP, and common MP4/MOV signatures and structural
  validators.
- Avoid promising filenames or folder structure from carving.
- Stream to a separate disk and provide partial/corrupt status.

### M6: Desktop UI (2–4 weeks)
- External-drive selection with internal disks locked, a clear read-only
  warning, and disk images as the preferred recovery input.
- Existing and discovered lost-partition candidates, scan modes, file filtering,
  preview, export, and logs on separate workflow pages.
- Pause/resume with durable checkpoints.
- No network access required.

### M7: Expanded support (ongoing)
- exFAT/FAT32; SSD/NVMe source imaging and TRIM warnings.
- Additional filesystem support and improved fragmented-file handling.
- Read-only device access via platform-specific authorized helpers. Never run
  the desktop UI as root or change device permissions to bypass authorization.

## CLI sketch
```
diskrescue inspect /images/diskrescue.img --json
diskrescue partitions /images/diskrescue.img --scan-lost
diskrescue scan /images/diskrescue.img --filesystem ntfs --checkpoint scan.db
diskrescue carve /images/diskrescue.img --types jpg,png,pdf,zip --checkpoint carve.db
diskrescue export --session scan.db --selected ids.txt --destination /recovered
```
`diskrescue inspect` is implemented for read-only MBR and primary GPT
inspection. The remaining commands are design proposals.

## Security and reliability requirements
- Image parsing assumes hostile/corrupt inputs; bounded allocations, integer overflow protection, no panics from malformed metadata.
- Export never overwrites existing user files without explicit consent.
- Reject path traversal, symlinks and unsafe filenames during export.
- Unit tests for truncated/corrupt structures and fuzzing for parsers.
- Compare recovery output against known-good synthetic disk images and established tools.
- Clear statuses: verified, likely, partial, unrecoverable/overwritten, unknown.
- Disk source always read-only; no destructive repair commands.

## Acceptance criteria for MVP
- Parse valid MBR/GPT images, including backup GPT when primary is damaged.
- Discover synthetic NTFS/ext4 partition signatures with documented confidence.
- Carve complete JPEG/PNG/PDF/ZIP test fixtures without modifying input image.
- Recover test data to a separate destination and verify hashes.
- Show precise warnings when data is incomplete or unsupported.

## Immediate next steps
1. Safely image the affected HDD.
2. Create synthetic test images with known NTFS and ext4 files.
3. Implement the read-only image reader and MBR/GPT parser.
4. Add lost-partition scanning and test it on synthetic images.
5. Only then analyze a copy of the affected HDD image.
