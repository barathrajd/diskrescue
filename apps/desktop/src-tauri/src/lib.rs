#![forbid(unsafe_code)]

use serde::Serialize;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

static LOST_SCAN_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalSource {
    id: String,
    model: Option<String>,
    serial: Option<String>,
    capacity_bytes: u64,
    mount_state_known: bool,
    system_state_known: bool,
    is_system: bool,
    external_state_known: bool,
    is_external: bool,
    mount_points: Vec<String>,
    eligibility_reason: Option<String>,
}

fn source_imaging_blocker(source: &PhysicalSource) -> Option<&'static str> {
    if !source.external_state_known {
        Some("External drive status unavailable")
    } else if !source.is_external {
        Some("Internal drive is locked")
    } else if source.id.trim().is_empty() {
        Some("Device identifier unavailable")
    } else if source.model.as_deref().is_none_or(str::is_empty) {
        Some("Model unavailable")
    } else if source.serial.as_deref().is_none_or(str::is_empty) {
        Some("Serial unavailable")
    } else if source.capacity_bytes == 0 {
        Some("Capacity unavailable")
    } else if !source.system_state_known {
        Some("System disk state unavailable")
    } else if source.is_system {
        Some("Source is a system disk")
    } else if !source.mount_state_known {
        Some("Mount state unavailable")
    } else if !source.mount_points.is_empty() {
        Some("Source is mounted")
    } else {
        None
    }
}

fn make_physical_source(
    id: String,
    model: Option<String>,
    serial: Option<String>,
    capacity_bytes: u64,
    mount_state_known: bool,
    system_state_known: bool,
    is_system: bool,
    external_state_known: bool,
    is_external: bool,
    mount_points: Vec<String>,
) -> PhysicalSource {
    let mut source = PhysicalSource {
        id,
        model,
        serial,
        capacity_bytes,
        mount_state_known,
        system_state_known,
        is_system,
        external_state_known,
        is_external,
        mount_points,
        eligibility_reason: None,
    };
    source.eligibility_reason = source_imaging_blocker(&source).map(str::to_owned);
    source
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_sources(inventory: &str) -> Result<Vec<PhysicalSource>, String> {
    let root: serde_json::Value = serde_json::from_str(inventory)
        .map_err(|error| format!("Could not parse Linux device inventory: {error}"))?;
    let disks = root
        .get("blockdevices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Linux device inventory did not include block devices".to_owned())?;
    let mut sources = Vec::new();

    for disk in disks {
        if disk.get("type").and_then(serde_json::Value::as_str) != Some("disk") {
            continue;
        }
        let name = disk
            .get("path")
            .and_then(serde_json::Value::as_str)
            .or_else(|| disk.get("name").and_then(serde_json::Value::as_str))
            .unwrap_or_default();
        let id = if name.starts_with("/dev/") {
            name.to_owned()
        } else if name.is_empty() {
            String::new()
        } else {
            format!("/dev/{name}")
        };
        let mut mount_points = Vec::new();
        collect_linux_mount_points(disk, &mut mount_points);
        let is_system = mount_points
            .iter()
            .any(|mount| mount == "/" || mount == "/boot" || mount.starts_with("/boot/"));
        let removable = disk.get("rm").and_then(serde_json::Value::as_bool);
        let transport = json_string(disk, "tran").map(|value| value.to_ascii_lowercase());
        let external_state_known = removable.is_some() || transport.is_some();
        let is_external = removable == Some(true)
            || transport.as_deref().is_some_and(|value| {
                matches!(value, "usb" | "firewire" | "thunderbolt" | "sd" | "mmc")
            });
        if !external_state_known || !is_external {
            continue;
        }
        sources.push(make_physical_source(
            id,
            json_string(disk, "model"),
            json_string(disk, "serial"),
            disk.get("size")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            linux_mount_state_known(disk),
            true,
            is_system,
            external_state_known,
            is_external,
            mount_points,
        ));
    }

    Ok(sources)
}

#[cfg(any(target_os = "linux", test))]
fn linux_mount_state_known(device: &serde_json::Value) -> bool {
    device.get("mountpoints").is_some()
        && device
            .get("children")
            .and_then(serde_json::Value::as_array)
            .is_none_or(|children| children.iter().all(linux_mount_state_known))
}

#[cfg(any(target_os = "linux", test))]
fn collect_linux_mount_points(device: &serde_json::Value, output: &mut Vec<String>) {
    if let Some(mounts) = device.get("mountpoints") {
        match mounts {
            serde_json::Value::String(mount) if !mount.is_empty() => output.push(mount.clone()),
            serde_json::Value::Array(mounts) => {
                output.extend(mounts.iter().filter_map(|mount| {
                    mount
                        .as_str()
                        .filter(|path| !path.is_empty())
                        .map(str::to_owned)
                }));
            }
            _ => {}
        }
    }
    if let Some(children) = device.get("children").and_then(serde_json::Value::as_array) {
        for child in children {
            collect_linux_mount_points(child, output);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_sources(inventory: &str) -> Result<Vec<PhysicalSource>, String> {
    let root: serde_json::Value = serde_json::from_str(inventory)
        .map_err(|error| format!("Could not parse Windows device inventory: {error}"))?;
    let disks: Vec<&serde_json::Value> = match &root {
        serde_json::Value::Array(disks) => disks.iter().collect(),
        serde_json::Value::Object(_) => vec![&root],
        _ => return Err("Windows device inventory did not contain disk records".to_owned()),
    };
    let mut sources = Vec::new();

    for disk in disks {
        let bus_type = json_string(disk, "BusType").map(|value| value.to_ascii_lowercase());
        let external_state_known = bus_type.is_some()
            || disk
                .get("BusType")
                .and_then(serde_json::Value::as_u64)
                .is_some();
        let is_external = bus_type
            .as_deref()
            .is_some_and(|value| matches!(value, "usb" | "sd" | "mmc" | "ieee1394" | "1394"))
            || disk
                .get("BusType")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|value| matches!(value, 7 | 12 | 13 | 15));
        if !external_state_known || !is_external {
            continue;
        }
        let Some(number) = disk.get("Number").and_then(serde_json::Value::as_u64) else {
            continue;
        };
        let mut mount_points = Vec::new();
        if let Some(partitions) = disk.get("Partitions").and_then(serde_json::Value::as_array) {
            for partition in partitions {
                if let Some(letter) = partition
                    .get("DriveLetter")
                    .and_then(serde_json::Value::as_str)
                    .filter(|letter| !letter.is_empty())
                {
                    mount_points.push(format!("{letter}:\\"));
                }
                if let Some(paths) = partition
                    .get("AccessPaths")
                    .and_then(serde_json::Value::as_array)
                {
                    mount_points.extend(paths.iter().filter_map(|path| {
                        path.as_str()
                            .filter(|path| !path.is_empty())
                            .map(str::to_owned)
                    }));
                }
            }
        }
        mount_points.sort();
        mount_points.dedup();
        let is_system = disk
            .get("IsBoot")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            || disk
                .get("IsSystem")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
        let system_state_known = disk
            .get("IsBoot")
            .and_then(serde_json::Value::as_bool)
            .is_some()
            && disk
                .get("IsSystem")
                .and_then(serde_json::Value::as_bool)
                .is_some();
        let mount_state_known = disk
            .get("PartitionStateKnown")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or_else(|| {
                disk.get("Partitions")
                    .and_then(serde_json::Value::as_array)
                    .is_some()
            });
        sources.push(make_physical_source(
            format!("PhysicalDrive{number}"),
            json_string(disk, "FriendlyName"),
            json_string(disk, "SerialNumber"),
            disk.get("Size")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            mount_state_known,
            system_state_known,
            is_system,
            external_state_known,
            is_external,
            mount_points,
        ));
    }

    Ok(sources)
}

#[tauri::command]
fn list_physical_sources() -> Result<Vec<PhysicalSource>, String> {
    #[cfg(target_os = "macos")]
    let mut sources = list_macos_sources()?;
    #[cfg(target_os = "linux")]
    let mut sources = list_linux_sources()?;
    #[cfg(target_os = "windows")]
    let mut sources = list_windows_sources()?;
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let mut sources = Vec::new();

    for source in &mut sources {
        source.eligibility_reason = source_imaging_blocker(source).map(str::to_owned);
    }
    Ok(sources)
}

#[cfg(target_os = "linux")]
fn list_linux_sources() -> Result<Vec<PhysicalSource>, String> {
    let output = Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "--tree",
            "--output",
            "NAME,PATH,TYPE,SIZE,MODEL,SERIAL,MOUNTPOINTS,RM,TRAN",
        ])
        .output()
        .map_err(|_| "Could not start lsblk to list physical disks".to_owned())?;
    if !output.status.success() {
        return Err("lsblk could not list physical disks".to_owned());
    }
    let inventory = String::from_utf8(output.stdout)
        .map_err(|_| "lsblk returned invalid device metadata".to_owned())?;
    parse_linux_sources(&inventory)
}

#[cfg(target_os = "windows")]
fn list_windows_sources() -> Result<Vec<PhysicalSource>, String> {
    const SCRIPT: &str = "$disks = Get-Disk | ForEach-Object { $disk = $_; $parts = @(); $partitionStateKnown = $true; try { $parts = @(Get-Partition -DiskNumber $disk.Number -ErrorAction Stop | Select-Object DriveLetter, AccessPaths) } catch { $partitionStateKnown = $false }; [pscustomobject]@{ Number = $disk.Number; FriendlyName = $disk.FriendlyName; SerialNumber = $disk.SerialNumber; Size = $disk.Size; BusType = [string]$disk.BusType; IsBoot = $disk.IsBoot; IsSystem = $disk.IsSystem; PartitionStateKnown = $partitionStateKnown; Partitions = $parts } }; ConvertTo-Json -InputObject @($disks) -Depth 4 -Compress";
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            SCRIPT,
        ])
        .output()
        .map_err(|_| "Could not start PowerShell to list physical disks".to_owned())?;
    if !output.status.success() {
        return Err("PowerShell could not list physical disks".to_owned());
    }
    let inventory = String::from_utf8(output.stdout)
        .map_err(|_| "PowerShell returned invalid device metadata".to_owned())?;
    parse_windows_sources(&inventory)
}

#[cfg(target_os = "macos")]
fn list_macos_sources() -> Result<Vec<PhysicalSource>, String> {
    let inventory = run_plist_command("diskutil", &["list", "-plist", "physical"])?;
    let whole_disks = inventory
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("WholeDisks"))
        .and_then(plist::Value::as_array)
        .ok_or_else(|| "diskutil did not return physical disk identifiers".to_owned())?;
    let partitions = inventory
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("AllDisksAndPartitions"))
        .and_then(plist::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let partition_inventory_known = inventory
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("AllDisksAndPartitions"))
        .and_then(plist::Value::as_array)
        .is_some();
    let system_info = run_plist_command("diskutil", &["info", "-plist", "/"]).ok();
    let system_disk = system_info.as_ref().and_then(|info| {
        info.as_dictionary()
            .and_then(|dictionary| plist_string(dictionary, "ParentWholeDisk"))
    });
    let mut sources = Vec::new();

    for identifier in whole_disks.iter().filter_map(plist::Value::as_string) {
        if !valid_macos_disk_identifier(identifier) {
            continue;
        }
        let device_path = format!("/dev/{identifier}");
        let info = run_plist_command("diskutil", &["info", "-plist", &device_path]).ok();
        let dictionary = info.as_ref().and_then(plist::Value::as_dictionary);
        let mut mount_points = Vec::new();
        let mut mount_state_known = partition_inventory_known;
        for partition_id in macos_partition_ids(&partitions, identifier) {
            let partition_path = format!("/dev/{partition_id}");
            match run_plist_command("diskutil", &["info", "-plist", &partition_path]) {
                Ok(partition_info) => {
                    if let Some(mount) = partition_info
                        .as_dictionary()
                        .and_then(|dictionary| plist_string(dictionary, "MountPoint"))
                    {
                        mount_points.push(mount);
                    }
                }
                Err(_) => mount_state_known = false,
            }
        }
        if let Some(mount) =
            dictionary.and_then(|dictionary| plist_string(dictionary, "MountPoint"))
        {
            mount_points.push(mount);
        }
        mount_points.sort();
        mount_points.dedup();

        let source_id = dictionary
            .and_then(|dictionary| plist_string(dictionary, "DeviceIdentifier"))
            .unwrap_or_else(|| identifier.to_owned());
        let model = dictionary.and_then(|dictionary| {
            plist_string(dictionary, "MediaName")
                .or_else(|| plist_string(dictionary, "Device / Media Name"))
        });
        let serial = dictionary.and_then(|dictionary| {
            plist_string(dictionary, "Serial Number")
                .or_else(|| plist_string(dictionary, "SerialNumber"))
        });
        let capacity_bytes = dictionary
            .and_then(|dictionary| {
                dictionary
                    .get("TotalSize")
                    .or_else(|| dictionary.get("Size"))
                    .and_then(plist::Value::as_unsigned_integer)
            })
            .unwrap_or(0);
        let internal = dictionary
            .and_then(|dictionary| dictionary.get("Internal"))
            .and_then(plist::Value::as_boolean);
        let external_state_known = internal.is_some();
        let is_external = internal == Some(false);
        if !external_state_known || !is_external {
            continue;
        }
        sources.push(make_physical_source(
            source_id.clone(),
            model,
            serial,
            capacity_bytes,
            mount_state_known,
            system_disk.is_some(),
            system_disk.as_deref() == Some(identifier)
                || system_disk.as_deref() == Some(&source_id),
            external_state_known,
            is_external,
            mount_points,
        ));
    }

    Ok(sources)
}

#[cfg(target_os = "macos")]
fn run_plist_command(command: &str, args: &[&str]) -> Result<plist::Value, String> {
    use std::io::Cursor;

    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|_| format!("Could not start {command} for device metadata"))?;
    if !output.status.success() {
        return Err(format!("{command} could not read device metadata"));
    }
    plist::Value::from_reader(Cursor::new(output.stdout))
        .map_err(|_| format!("{command} returned invalid property-list data"))
}

#[cfg(target_os = "macos")]
fn plist_string(dictionary: &plist::Dictionary, key: &str) -> Option<String> {
    dictionary
        .get(key)
        .and_then(plist::Value::as_string)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(target_os = "macos")]
fn valid_macos_disk_identifier(identifier: &str) -> bool {
    identifier.strip_prefix("disk").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
    })
}

#[cfg(target_os = "macos")]
fn macos_partition_ids(inventory: &[plist::Value], disk_id: &str) -> Vec<String> {
    inventory
        .iter()
        .filter_map(plist::Value::as_dictionary)
        .filter(|disk| {
            disk.get("DeviceIdentifier")
                .and_then(plist::Value::as_string)
                == Some(disk_id)
        })
        .filter_map(|disk| disk.get("Partitions").and_then(plist::Value::as_array))
        .flat_map(|partitions| partitions.iter().filter_map(plist::Value::as_dictionary))
        .filter_map(|partition| plist_string(partition, "DeviceIdentifier"))
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageReview {
    image_bytes: u64,
    mbr_status: String,
    mbr_partition_count: usize,
    gpt_status: String,
    gpt_partition_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanReview {
    image_bytes: u64,
    mbr_error: Option<String>,
    gpt_error: Option<String>,
    partitions: Vec<PartitionReview>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PartitionReview {
    table: String,
    index: u32,
    type_label: String,
    offset_bytes: u64,
    length_bytes: u64,
    filesystem: Option<FilesystemReview>,
    issue: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilesystemReview {
    kind: String,
    offset_bytes: u64,
    length_bytes: u64,
    confidence: String,
    reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LostScanProgress {
    scanned_bytes: u64,
    total_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LostScanReview {
    image_bytes: u64,
    scanned_bytes: u64,
    complete: bool,
    candidates: Vec<PartitionReview>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeletedFileReview {
    record_number: u64,
    sequence_number: u16,
    is_directory: bool,
    name: Option<String>,
    parent_record_number: Option<u64>,
    data_size_bytes: Option<u64>,
}

#[tauri::command]
fn inspect_image(path: String) -> Result<ImageReview, String> {
    let inspection = recovery_core::inspect_image(path).map_err(|error| error.to_string())?;
    let gpt_source = inspection.gpt_source;
    let (mbr_status, mbr_partition_count) = match inspection.mbr {
        Ok(mbr) => ("Valid MBR".to_owned(), mbr.partitions.len()),
        Err(error) => (format!("Unreadable: {error}"), 0),
    };
    let (gpt_status, gpt_partition_count) = match inspection.gpt {
        Ok(gpt) => (
            if gpt_source == Some("backup") {
                "Valid backup GPT".to_owned()
            } else {
                "Valid GPT".to_owned()
            },
            gpt.partitions.len(),
        ),
        Err(error) if error == "GPT not indicated by a protective MBR" => {
            ("Not present".to_owned(), 0)
        }
        Err(error) => (format!("Unreadable: {error}"), 0),
    };

    Ok(ImageReview {
        image_bytes: inspection.image_bytes,
        mbr_status,
        mbr_partition_count,
        gpt_status,
        gpt_partition_count,
    })
}

#[tauri::command]
fn scan_image(path: String) -> Result<ScanReview, String> {
    let scan = recovery_core::scan_image(path).map_err(|error| error.to_string())?;
    let partitions = scan
        .partition_candidates
        .into_iter()
        .map(partition_review)
        .collect();
    Ok(ScanReview {
        image_bytes: scan.image_bytes,
        mbr_error: scan.mbr_error,
        gpt_error: scan.gpt_error,
        partitions,
    })
}

#[tauri::command]
fn scan_ntfs_deleted_files(
    path: String,
    partition_offset_bytes: u64,
    partition_length_bytes: u64,
) -> Result<Vec<DeletedFileReview>, String> {
    recovery_core::scan_ntfs_deleted_files(&path, partition_offset_bytes, partition_length_bytes)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| DeletedFileReview {
                    record_number: entry.record_number,
                    sequence_number: entry.sequence_number,
                    is_directory: entry.is_directory,
                    name: entry.name,
                    parent_record_number: entry.parent_record_number,
                    data_size_bytes: entry.data_size_bytes,
                })
                .collect()
        })
        .map_err(|error| error.to_string())
}

fn partition_review(partition: recovery_core::PartitionFinding) -> PartitionReview {
    PartitionReview {
        table: partition.table.to_owned(),
        index: partition.index,
        type_label: partition.type_label,
        offset_bytes: partition.offset_bytes,
        length_bytes: partition.length_bytes,
        filesystem: partition.filesystem.map(|filesystem| FilesystemReview {
            kind: match filesystem.kind {
                partition_hunter::FileSystemKind::Ntfs => "NTFS".to_owned(),
                partition_hunter::FileSystemKind::Ext4 => "ext4".to_owned(),
            },
            offset_bytes: filesystem.offset_bytes,
            length_bytes: filesystem.length_bytes,
            confidence: filesystem.confidence.to_owned(),
            reasons: filesystem.reasons.into_iter().map(str::to_owned).collect(),
        }),
        issue: partition.issue,
    }
}

#[tauri::command]
fn search_lost_filesystems(app: tauri::AppHandle, path: String) -> Result<LostScanReview, String> {
    LOST_SCAN_CANCELLED.store(false, Ordering::SeqCst);
    let total_bytes = std::fs::metadata(&path)
        .map_err(|error| format!("Could not read image metadata: {error}"))?
        .len();
    let _ = app.emit(
        "lost-filesystem-progress",
        LostScanProgress {
            scanned_bytes: 0,
            total_bytes,
        },
    );
    let mut last_reported = 0_u64;
    let scan = recovery_core::search_lost_filesystems(path, |scanned_bytes, total_bytes| {
        if scanned_bytes == total_bytes
            || scanned_bytes.saturating_sub(last_reported) >= 32 * 1024 * 1024
        {
            let _ = app.emit(
                "lost-filesystem-progress",
                LostScanProgress {
                    scanned_bytes,
                    total_bytes,
                },
            );
            last_reported = scanned_bytes;
        }
        !LOST_SCAN_CANCELLED.load(Ordering::SeqCst)
    })
    .map_err(|error| error.to_string())?;

    Ok(LostScanReview {
        image_bytes: scan.image_bytes,
        scanned_bytes: scan.scanned_bytes,
        complete: scan.complete,
        candidates: scan.candidates.into_iter().map(partition_review).collect(),
    })
}

#[tauri::command]
fn cancel_lost_filesystem_search() {
    LOST_SCAN_CANCELLED.store(true, Ordering::SeqCst);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            inspect_image,
            list_physical_sources,
            scan_image,
            scan_ntfs_deleted_files,
            search_lost_filesystems,
            cancel_lost_filesystem_search
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DiskRescue");
}

#[cfg(test)]
mod tests {
    use super::{
        PhysicalSource, inspect_image, parse_linux_sources, parse_windows_sources,
        source_imaging_blocker,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn inspecting_selected_image_returns_partition_summary() {
        let path = temporary_image_path();
        let mut image = vec![0_u8; 512];
        image[510] = 0x55;
        image[511] = 0xaa;
        fs::write(&path, image).unwrap();

        let result = inspect_image(path.to_str().unwrap().to_owned()).unwrap();

        assert_eq!(result.image_bytes, 512);
        assert_eq!(result.mbr_partition_count, 0);
        assert_eq!(result.mbr_status, "Valid MBR");
        assert_eq!(result.gpt_status, "Not present");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn source_imaging_is_blocked_when_identity_or_mount_state_is_unsafe() {
        let mut source = PhysicalSource {
            id: "/dev/disk4".to_owned(),
            model: Some("External HDD".to_owned()),
            serial: Some("SERIAL-4".to_owned()),
            capacity_bytes: 1_000,
            mount_state_known: true,
            system_state_known: true,
            is_system: false,
            external_state_known: true,
            is_external: true,
            mount_points: Vec::new(),
            eligibility_reason: None,
        };

        assert_eq!(source_imaging_blocker(&source), None);
        source.serial = None;
        assert_eq!(source_imaging_blocker(&source), Some("Serial unavailable"));
        source.serial = Some("SERIAL-4".to_owned());
        source.mount_points.push("/Volumes/Recovery".to_owned());
        assert_eq!(source_imaging_blocker(&source), Some("Source is mounted"));
        source.mount_points.clear();
        source.is_system = true;
        assert_eq!(
            source_imaging_blocker(&source),
            Some("Source is a system disk")
        );
        source.is_system = false;
        source.is_external = false;
        assert_eq!(
            source_imaging_blocker(&source),
            Some("Internal drive is locked")
        );
        source.external_state_known = false;
        assert_eq!(
            source_imaging_blocker(&source),
            Some("External drive status unavailable")
        );
    }

    #[test]
    fn parses_linux_block_device_inventory_with_child_mounts() {
        let inventory = r#"{"blockdevices":[{"name":"sda","path":"/dev/sda","type":"disk","size":5000,"model":"Internal SSD","serial":"SERIAL-A","rm":false,"tran":"sata","mountpoints":["/"]},{"name":"sdb","path":"/dev/sdb","type":"disk","size":1000,"model":"External HDD","serial":"SERIAL-B","rm":true,"tran":"usb","mountpoints":[null],"children":[{"name":"sdb1","path":"/dev/sdb1","type":"part","size":1000,"mountpoints":["/media/user/RECOVERY"]}]}]}"#;

        let sources = parse_linux_sources(inventory).unwrap();

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, "/dev/sdb");
        assert_eq!(sources[0].serial.as_deref(), Some("SERIAL-B"));
        assert!(sources[0].is_external);
        assert_eq!(sources[0].mount_points, ["/media/user/RECOVERY"]);
        assert_eq!(
            source_imaging_blocker(&sources[0]),
            Some("Source is mounted")
        );
    }

    #[test]
    fn hides_linux_disks_when_external_status_is_unknown() {
        let inventory = r#"{"blockdevices":[{"name":"sdz","type":"disk","size":1000,"model":"Unclassified disk","mountpoints":[null]}]}"#;
        assert!(parse_linux_sources(inventory).unwrap().is_empty());
    }

    #[test]
    fn parses_windows_disk_inventory_and_system_flags() {
        let inventory = r#"[{"Number":0,"FriendlyName":"Internal SSD","SerialNumber":"SERIAL-I","Size":4000,"BusType":"NVMe","IsBoot":true,"IsSystem":true,"Partitions":[{"DriveLetter":"C"}]},{"Number":2,"FriendlyName":"External HDD","SerialNumber":"SERIAL-C","Size":2000,"BusType":"USB","IsBoot":false,"IsSystem":true,"Partitions":[{"DriveLetter":"E"}]}]"#;

        let sources = parse_windows_sources(inventory).unwrap();

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, "PhysicalDrive2");
        assert_eq!(sources[0].serial.as_deref(), Some("SERIAL-C"));
        assert!(sources[0].is_system);
        assert!(sources[0].is_external);
        assert_eq!(
            source_imaging_blocker(&sources[0]),
            Some("Source is a system disk")
        );

        let inventory_without_partition_state = r#"{"Number":3,"FriendlyName":"External HDD","SerialNumber":"SERIAL-D","Size":3000,"BusType":"USB","IsBoot":false,"IsSystem":false,"PartitionStateKnown":false,"Partitions":[]}"#;
        let unknown_mount = parse_windows_sources(inventory_without_partition_state).unwrap();
        assert_eq!(
            source_imaging_blocker(&unknown_mount[0]),
            Some("Mount state unavailable")
        );

        let unknown_bus_type = r#"{"Number":4,"FriendlyName":"Unclassified disk","SerialNumber":"SERIAL-E","Size":4000,"IsBoot":false,"IsSystem":false,"Partitions":[]}"#;
        assert!(parse_windows_sources(unknown_bus_type).unwrap().is_empty());
    }

    fn temporary_image_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("diskrescue-test-{nonce}.img"))
    }
}
