# Physical HDD imaging design

Keep the Tauri app unprivileged while letting the user create a read-only image
of an identified secondary HDD. The privileged component can only inspect
devices and copy one selected device to a new image on another drive.

## Context

The confirmed recovery plan requires both existing-image selection and
physical-device imaging on Windows, macOS, and Linux. The app already selects
and inspects existing image files. A local macOS probe showed that a normal user
cannot read a raw disk device; Microsoft's [physical-disk API
requirements](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea)
require administrator privileges, and Linux's [`open(2)` permissions](https://man7.org/linux/man-pages/man2/open.2.html)
apply to raw device files. Running the whole Tauri webview with elevated
privileges would expose much more authority than imaging needs.

## Goals / Non-goals

The imaging path must preserve the existing image workflow and fail closed
when it cannot positively identify or safely read the source.

**Goals**

- Keep the desktop UI and recovery parser at the user's normal privilege level.
- Confirm source model, serial, capacity, system-disk state, and mount state
  before imaging.
- Open the source for reads only and create a new image on a different physical
  device without replacing an existing file.
- Report progress, cancellation, read errors, and incomplete images.
- Keep existing-image recovery available when OS authorization or source details
  are unavailable.

**Non-goals**

- Elevating the full Tauri app or exposing a generic privileged file or shell
  command.
- Repairing, mounting, formatting, initializing, or writing to source media.
- Imaging the active system or boot disk, mounted source volumes, RAID sets, or
  devices whose identity cannot be confirmed.
- Supporting every historical OS release for direct physical imaging.

## Proposed design

Use a small, OS-specific privileged helper behind a Rust `DiskImager` interface.
The UI requests a device inventory, displays identity and safety state, and
requires explicit confirmation of the selected source and destination. The
helper re-enumerates the source immediately before capture, verifies the
identity and safety conditions again, opens it read-only, and copies it in
bounded chunks to an exclusively created image file on a different device.
The helper never accepts arbitrary commands or general read/write requests.

The data passed across the boundary is limited to these operations:

```text
list_sources() -> [DeviceDescriptor]
create_image(source_id, destination_path) -> ImageJobId
cancel_image(job_id) -> Result
image_progress(job_id) -> ImageProgress
```

`DeviceDescriptor` contains an opaque device ID, model, serial, capacity,
removable/internal state, system-disk state, and mounted-volume details.
`ImageProgress` contains bytes read, total bytes, current status, and a bounded
error summary. The helper validates both the source ID and destination path;
the destination must be on a different device, must not exist, and must not
resolve through a symlink. A cancelled or failed capture remains labeled
incomplete and can never be scanned as a complete image without an explicit
user choice.

Windows uses a separate helper launched with UAC only for imaging operations;
Microsoft recommends separating elevated operations into a helper process in
its [administrator-privilege guidance](https://learn.microsoft.com/en-us/windows/win32/secbp/running-with-administrator-privileges).
Linux uses a narrowly authorized helper through the desktop's [polkit
authorization service](https://www.freedesktop.org/software/polkit/docs/latest/polkit.8)
when available. macOS uses a signed launch daemon registered through
[`SMAppService`](https://developer.apple.com/documentation/servicemanagement/smappservice)
on macOS 13 and later. Older macOS versions retain existing-image support and
use the external-imaging fallback. Each helper uses the platform's device
inventory APIs and returns no serial number as an empty value; imaging is
disabled when required identity fields are unavailable.

## Alternatives considered

Running the entire Tauri app as administrator or root was rejected because the
webview and parser would inherit system-wide access. Using an in-process raw
device API was rejected because it still requires elevated access and would
leave that authority inside the UI process. Requiring an external imaging tool
for every user is simpler, but it does not meet the confirmed app-side imaging
goal; it remains the fallback on platforms where the helper cannot be safely
installed or authorized.

## Risks & mitigations

These risks need explicit checks in the helper protocol and release process.

- **Helper signing and installation differ by OS.** Validate packaged helpers
  on each target OS before enabling the imaging action; otherwise keep the
  existing-image path and explain the external imaging step.
- **Device identity may not include a serial number.** Display all available
  identity fields and disable direct imaging unless model, serial, and capacity
  are confirmed.
- **A privileged IPC request could be malformed or replayed.** Revalidate
  source identity and destination policy inside the helper for every capture;
  expose only bounded operations and reject unknown request fields.
- **Source reads may fail or return short data.** Stop on errors, retain an
  incomplete status, and never represent unread sectors as recovered content.
- **A source or destination can change during selection.** Recheck device IDs,
  capacity, destination volume, and mount state immediately before opening
  handles.

## Rollout & rollback

Keep helper imaging disabled until per-platform safety tests and signed package
checks pass. Enable it only for the platforms and OS versions with verified
device identification, read-only access, and separate-destination enforcement.
If any check fails, remove or disable the helper entry point; the unprivileged
existing-image workflow remains available without data migration or cleanup.
