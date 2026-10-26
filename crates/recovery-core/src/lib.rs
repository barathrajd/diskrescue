#![forbid(unsafe_code)]

use disk_reader::{ImageReader, ReadAt};
use fs_ntfs::NtfsFileRecord;
use partition_hunter::{FileSystemCandidate, identify_filesystem};
use partition_parser::{Gpt, Mbr, parse_backup_gpt, parse_gpt, parse_mbr};
use std::io;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::{
        RecoveryScan, inspect_image, scan_image, scan_ntfs_deleted_files, search_lost_filesystems,
    };
    use partition_hunter::FileSystemKind;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn scans_partition_table_entries_for_supported_filesystems() {
        let path = temporary_image_path();
        let mut image = vec![0_u8; 33 * 512];
        image[446 + 4] = 0x07;
        image[446 + 8..446 + 12].copy_from_slice(&1_u32.to_le_bytes());
        image[446 + 12..446 + 16].copy_from_slice(&32_u32.to_le_bytes());
        image[510..512].copy_from_slice(&[0x55, 0xaa]);
        let boot = &mut image[512..1024];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[11..13].copy_from_slice(&512_u16.to_le_bytes());
        boot[13] = 8;
        boot[40..48].copy_from_slice(&32_u64.to_le_bytes());
        boot[48..56].copy_from_slice(&2_u64.to_le_bytes());
        boot[56..64].copy_from_slice(&3_u64.to_le_bytes());
        boot[64] = 1;
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);
        fs::write(&path, image).unwrap();

        let scan: RecoveryScan = scan_image(&path).unwrap();

        assert_eq!(scan.image_bytes, 33 * 512);
        assert_eq!(scan.partition_candidates.len(), 1);
        let partition = &scan.partition_candidates[0];
        assert_eq!(partition.offset_bytes, 512);
        assert_eq!(partition.length_bytes, 32 * 512);
        assert_eq!(
            partition.filesystem.as_ref().unwrap().kind,
            FileSystemKind::Ntfs
        );
        assert!(partition.issue.is_none());
        let lost_scan = search_lost_filesystems(&path, |_, _| true).unwrap();
        assert!(lost_scan.candidates.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn finds_a_volume_from_backup_gpt_when_the_primary_header_is_damaged() {
        let path = temporary_image_path();
        let mut image = vec![0_u8; 256 * 512];
        image[446 + 4] = 0xee;
        image[446 + 8..446 + 12].copy_from_slice(&1_u32.to_le_bytes());
        image[446 + 12..446 + 16].copy_from_slice(&255_u32.to_le_bytes());
        image[510..512].copy_from_slice(&[0x55, 0xaa]);

        let mut entry = [0_u8; 128];
        entry[0] = 1;
        entry[16] = 2;
        entry[32..40].copy_from_slice(&34_u64.to_le_bytes());
        entry[40..48].copy_from_slice(&100_u64.to_le_bytes());
        let entries_crc = test_crc32(&entry);
        image[223 * 512..223 * 512 + 128].copy_from_slice(&entry);

        let ext4 = &mut image[34 * 512 + 1024..34 * 512 + 2048];
        ext4[0..4].copy_from_slice(&32_u32.to_le_bytes());
        ext4[4..8].copy_from_slice(&32_u32.to_le_bytes());
        ext4[24..28].copy_from_slice(&0_u32.to_le_bytes());
        ext4[32..36].copy_from_slice(&16_u32.to_le_bytes());
        ext4[0x38..0x3a].copy_from_slice(&[0x53, 0xef]);

        let header = &mut image[255 * 512..256 * 512];
        header[0..8].copy_from_slice(b"EFI PART");
        header[8..12].copy_from_slice(&0x0001_0000_u32.to_le_bytes());
        header[12..16].copy_from_slice(&92_u32.to_le_bytes());
        header[24..32].copy_from_slice(&255_u64.to_le_bytes());
        header[32..40].copy_from_slice(&1_u64.to_le_bytes());
        header[40..48].copy_from_slice(&34_u64.to_le_bytes());
        header[48..56].copy_from_slice(&222_u64.to_le_bytes());
        header[72..80].copy_from_slice(&223_u64.to_le_bytes());
        header[80..84].copy_from_slice(&1_u32.to_le_bytes());
        header[84..88].copy_from_slice(&128_u32.to_le_bytes());
        header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let mut checksum_header = header[..92].to_vec();
        checksum_header[16..20].fill(0);
        header[16..20].copy_from_slice(&test_crc32(&checksum_header).to_le_bytes());
        fs::write(&path, image).unwrap();

        let scan = scan_image(&path).unwrap();

        assert_eq!(scan.partition_candidates.len(), 1);
        let partition = &scan.partition_candidates[0];
        assert_eq!(partition.table, "GPT backup");
        assert_eq!(partition.offset_bytes, 34 * 512);
        assert_eq!(
            partition.filesystem.as_ref().unwrap().kind,
            FileSystemKind::Ext4
        );
        assert!(
            scan.gpt_error
                .as_deref()
                .unwrap()
                .contains("using backup GPT")
        );
        let inspection = inspect_image(&path).unwrap();
        assert_eq!(inspection.gpt_source, Some("backup"));
        assert_eq!(inspection.gpt.unwrap().partitions.len(), 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn finds_a_lost_ext4_start_without_a_partition_table() {
        let path = temporary_image_path();
        let mut image = vec![0_u8; 64 * 1024];
        let superblock = &mut image[4096 + 1024..4096 + 2048];
        superblock[0..4].copy_from_slice(&32_u32.to_le_bytes());
        superblock[4..8].copy_from_slice(&32_u32.to_le_bytes());
        superblock[24..28].copy_from_slice(&0_u32.to_le_bytes());
        superblock[32..36].copy_from_slice(&16_u32.to_le_bytes());
        superblock[0x38..0x3a].copy_from_slice(&[0x53, 0xef]);
        fs::write(&path, image).unwrap();

        let scan = search_lost_filesystems(&path, |_, _| true).unwrap();

        assert!(scan.complete);
        assert_eq!(scan.candidates.len(), 1);
        assert_eq!(scan.candidates[0].offset_bytes, 4096);
        assert_eq!(
            scan.candidates[0].filesystem.as_ref().unwrap().kind,
            FileSystemKind::Ext4
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn finds_a_lost_ntfs_start_on_a_512_byte_boundary() {
        let path = temporary_image_path();
        let mut image = vec![0_u8; 64 * 1024];
        let boot = &mut image[4096..4608];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[11..13].copy_from_slice(&512_u16.to_le_bytes());
        boot[13] = 8;
        boot[40..48].copy_from_slice(&32_u64.to_le_bytes());
        boot[48..56].copy_from_slice(&2_u64.to_le_bytes());
        boot[56..64].copy_from_slice(&3_u64.to_le_bytes());
        boot[64] = 1;
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);
        fs::write(&path, image).unwrap();

        let scan = search_lost_filesystems(&path, |_, _| true).unwrap();

        assert_eq!(scan.candidates.len(), 1);
        assert_eq!(scan.candidates[0].offset_bytes, 4096);
        assert_eq!(
            scan.candidates[0].filesystem.as_ref().unwrap().kind,
            FileSystemKind::Ntfs
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn scans_deleted_ntfs_entries_from_a_selected_partition_in_the_image() {
        let path = temporary_image_path();
        let image = synthetic_ntfs_image_with_deleted_file();
        fs::write(&path, image).unwrap();

        let entries = scan_ntfs_deleted_files(&path, 0, 16 * 512).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].record_number, 1);
        assert_eq!(entries[0].name.as_deref(), Some("lost.mp4"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn lost_filesystem_search_returns_partial_results_when_cancelled() {
        let path = temporary_image_path();
        let mut image = vec![0_u8; 64 * 1024];
        let superblock = &mut image[4096 + 1024..4096 + 2048];
        superblock[0..4].copy_from_slice(&32_u32.to_le_bytes());
        superblock[4..8].copy_from_slice(&32_u32.to_le_bytes());
        superblock[24..28].copy_from_slice(&0_u32.to_le_bytes());
        superblock[32..36].copy_from_slice(&16_u32.to_le_bytes());
        superblock[0x38..0x3a].copy_from_slice(&[0x53, 0xef]);
        fs::write(&path, image).unwrap();

        let scan = search_lost_filesystems(&path, |_, _| false).unwrap();

        assert!(!scan.complete);
        assert_eq!(scan.candidates.len(), 1);
        fs::remove_file(path).unwrap();
    }

    fn test_crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0_u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = 0_u32.wrapping_sub(crc & 1);
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }

    fn synthetic_ntfs_image_with_deleted_file() -> Vec<u8> {
        let mut image = vec![0_u8; 16 * 512];
        let boot = &mut image[..512];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[11..13].copy_from_slice(&512_u16.to_le_bytes());
        boot[13] = 1;
        boot[40..48].copy_from_slice(&16_u64.to_le_bytes());
        boot[48..56].copy_from_slice(&2_u64.to_le_bytes());
        boot[56..64].copy_from_slice(&3_u64.to_le_bytes());
        boot[64] = (-10_i8) as u8;
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);

        let mut base = synthetic_file_record("$MFT", 5, true);
        base[56 + 0..56 + 4].copy_from_slice(&0x80_u32.to_le_bytes());
        base[56 + 4..56 + 8].copy_from_slice(&80_u32.to_le_bytes());
        base[56 + 8] = 1;
        base[56 + 32..56 + 34].copy_from_slice(&64_u16.to_le_bytes());
        base[56 + 48..56 + 56].copy_from_slice(&2048_u64.to_le_bytes());
        base[56 + 64..56 + 68].copy_from_slice(&[0x11, 4, 2, 0]);
        base[24..28].copy_from_slice(&140_u32.to_le_bytes());
        base[136..140].copy_from_slice(&u32::MAX.to_le_bytes());
        image[2 * 512..2 * 512 + 1024].copy_from_slice(&base);
        let deleted = synthetic_file_record("lost.mp4", 5, false);
        image[4 * 512..4 * 512 + 1024].copy_from_slice(&deleted);
        image
    }

    fn synthetic_file_record(name: &str, parent_record: u64, in_use: bool) -> [u8; 1024] {
        let mut record = [0_u8; 1024];
        record[0..4].copy_from_slice(b"FILE");
        record[4..6].copy_from_slice(&48_u16.to_le_bytes());
        record[6..8].copy_from_slice(&3_u16.to_le_bytes());
        record[48..50].copy_from_slice(&0xaaaa_u16.to_le_bytes());
        record[50..52].copy_from_slice(&0x1111_u16.to_le_bytes());
        record[52..54].copy_from_slice(&0x2222_u16.to_le_bytes());
        record[16..18].copy_from_slice(&7_u16.to_le_bytes());
        record[20..22].copy_from_slice(&56_u16.to_le_bytes());
        record[22..24].copy_from_slice(&(u16::from(in_use)).to_le_bytes());
        record[24..28].copy_from_slice(&172_u32.to_le_bytes());
        record[28..32].copy_from_slice(&1024_u32.to_le_bytes());
        let attribute = &mut record[56..168];
        attribute[0..4].copy_from_slice(&0x30_u32.to_le_bytes());
        attribute[4..8].copy_from_slice(&112_u32.to_le_bytes());
        attribute[16..20].copy_from_slice(&84_u32.to_le_bytes());
        attribute[20..22].copy_from_slice(&24_u16.to_le_bytes());
        let value = &mut attribute[24..108];
        value[0..8].copy_from_slice(&parent_record.to_le_bytes());
        let utf16_name = name.encode_utf16().collect::<Vec<_>>();
        value[64] = utf16_name.len() as u8;
        value[65] = 1;
        for (index, character) in utf16_name.iter().enumerate() {
            let offset = 66 + index * 2;
            value[offset..offset + 2].copy_from_slice(&character.to_le_bytes());
        }
        record[168..172].copy_from_slice(&u32::MAX.to_le_bytes());
        record[510..512].copy_from_slice(&0xaaaa_u16.to_le_bytes());
        record[1022..1024].copy_from_slice(&0xaaaa_u16.to_le_bytes());
        record
    }

    fn temporary_image_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("diskrescue-scan-{nonce}.img"))
    }
}

#[derive(Debug)]
pub struct ImageInspection {
    pub image_bytes: u64,
    pub mbr: Result<Mbr, String>,
    pub gpt: Result<Gpt, String>,
    pub gpt_source: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct PartitionFinding {
    pub table: &'static str,
    pub index: u32,
    pub type_label: String,
    pub offset_bytes: u64,
    pub length_bytes: u64,
    pub filesystem: Option<FileSystemCandidate>,
    pub issue: Option<String>,
}

#[derive(Debug)]
pub struct RecoveryScan {
    pub image_bytes: u64,
    pub partition_candidates: Vec<PartitionFinding>,
    pub mbr_error: Option<String>,
    pub gpt_error: Option<String>,
}

#[derive(Debug)]
pub struct LostFilesystemScan {
    pub image_bytes: u64,
    pub scanned_bytes: u64,
    pub complete: bool,
    pub candidates: Vec<PartitionFinding>,
}

/// Lists deleted NTFS entries from a selected partition in a disk image.
/// The image is opened read-only; offsets and lengths are relative to the
/// whole image and are revalidated by the NTFS reader.
pub fn scan_ntfs_deleted_files(
    path: impl AsRef<Path>,
    partition_offset_bytes: u64,
    partition_length_bytes: u64,
) -> io::Result<Vec<NtfsFileRecord>> {
    let image = ImageReader::open(path)?;
    scan_ntfs_deleted_files_from_reader(&image, partition_offset_bytes, partition_length_bytes)
}

/// Scans a selected NTFS partition through a read-only random-access source.
pub fn scan_ntfs_deleted_files_from_reader<R: ReadAt>(
    source: &R,
    partition_offset_bytes: u64,
    partition_length_bytes: u64,
) -> io::Result<Vec<NtfsFileRecord>> {
    fs_ntfs::scan_deleted_file_records(source, partition_offset_bytes, partition_length_bytes)
}

pub fn inspect_image(path: impl AsRef<Path>) -> io::Result<ImageInspection> {
    let image = ImageReader::open(path)?;
    let mbr = parse_mbr(&image).map_err(|error| error.to_string());
    let (gpt, gpt_source) = match &mbr {
        Ok(mbr) if mbr.protective => match parse_gpt(&image) {
            Ok(gpt) => (Ok(gpt), Some("primary")),
            Err(primary_error) => match parse_backup_gpt(&image) {
                Ok(gpt) => (Ok(gpt), Some("backup")),
                Err(backup_error) => (
                    Err(format!(
                        "Primary GPT unavailable: {primary_error}; backup GPT unavailable: {backup_error}"
                    )),
                    None,
                ),
            },
        },
        Ok(_) => (
            Err("GPT not indicated by a protective MBR".to_owned()),
            None,
        ),
        Err(error) => (Err(format!("GPT not checked: {error}")), None),
    };

    Ok(ImageInspection {
        image_bytes: image.len(),
        mbr,
        gpt,
        gpt_source,
    })
}

/// Scans partition-table entries in an image and checks each partition start
/// for supported NTFS or ext4 structures. This does not search unallocated
/// image regions for lost partition tables.
pub fn scan_image(path: impl AsRef<Path>) -> io::Result<RecoveryScan> {
    let image = ImageReader::open(path)?;
    let mbr = parse_mbr(&image);
    let should_check_gpt = !matches!(&mbr, Ok(mbr) if !mbr.protective);
    let (gpt, gpt_table, gpt_error) = if should_check_gpt {
        match parse_gpt(&image) {
            Ok(gpt) => (Ok(gpt), "GPT", None),
            Err(primary_error) => match parse_backup_gpt(&image) {
                Ok(gpt) => (
                    Ok(gpt),
                    "GPT backup",
                    Some(format!(
                        "Primary GPT unavailable: {primary_error}; using backup GPT"
                    )),
                ),
                Err(backup_error) => {
                    let message = format!(
                        "Primary GPT unavailable: {primary_error}; backup GPT unavailable: {backup_error}"
                    );
                    (
                        Err(io::Error::new(io::ErrorKind::InvalidData, message.clone())),
                        "GPT backup",
                        Some(message),
                    )
                }
            },
        }
    } else {
        (
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "GPT not indicated by a protective MBR",
            )),
            "GPT",
            None,
        )
    };
    let mut partition_candidates = Vec::new();
    if let Ok(mbr) = &mbr {
        if !mbr.protective {
            for partition in &mbr.partitions {
                let offset_bytes =
                    u64::from(partition.first_lba)
                        .checked_mul(512)
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidData, "MBR offset overflows")
                        })?;
                let length_bytes =
                    u64::from(partition.sectors)
                        .checked_mul(512)
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidData, "MBR length overflows")
                        })?;
                partition_candidates.push(inspect_partition(
                    &image,
                    "MBR",
                    u32::from(partition.index),
                    format!("MBR type 0x{:02x}", partition.type_code),
                    offset_bytes,
                    length_bytes,
                ));
            }
        }
    }
    if let Ok(gpt) = &gpt {
        for partition in &gpt.partitions {
            let offset_bytes = partition.first_lba.checked_mul(512).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "GPT offset overflows")
            })?;
            let sector_count = partition
                .last_lba
                .checked_sub(partition.first_lba)
                .and_then(|sectors| sectors.checked_add(1))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "GPT partition length overflows")
                })?;
            let length_bytes = sector_count.checked_mul(512).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "GPT length overflows")
            })?;
            let guid = partition
                .type_guid
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            partition_candidates.push(inspect_partition(
                &image,
                gpt_table,
                partition.index,
                format!("GPT type {guid}"),
                offset_bytes,
                length_bytes,
            ));
        }
    }

    Ok(RecoveryScan {
        image_bytes: image.len(),
        partition_candidates,
        mbr_error: mbr.err().map(|error| error.to_string()),
        gpt_error,
    })
}

fn inspect_partition<R: ReadAt>(
    image: &R,
    table: &'static str,
    index: u32,
    type_label: String,
    offset_bytes: u64,
    length_bytes: u64,
) -> PartitionFinding {
    match identify_filesystem(image, offset_bytes, length_bytes) {
        Ok(Some(filesystem)) => PartitionFinding {
            table,
            index,
            type_label,
            offset_bytes,
            length_bytes,
            filesystem: Some(filesystem),
            issue: None,
        },
        Ok(None) => PartitionFinding {
            table,
            index,
            type_label,
            offset_bytes,
            length_bytes,
            filesystem: None,
            issue: Some("No supported filesystem found at partition start".to_owned()),
        },
        Err(error) => PartitionFinding {
            table,
            index,
            type_label,
            offset_bytes,
            length_bytes,
            filesystem: None,
            issue: Some(format!("Could not inspect partition: {error}")),
        },
    }
}

/// Searches a disk image at 512-byte boundaries for NTFS boot sectors and
/// ext4 superblocks. Reads in bounded chunks and skips known partition ranges.
/// Returning `false` from `report_progress` stops the scan and returns the
/// findings collected so far with `complete` set to `false`.
pub fn search_lost_filesystems<F>(
    path: impl AsRef<Path>,
    mut report_progress: F,
) -> io::Result<LostFilesystemScan>
where
    F: FnMut(u64, u64) -> bool,
{
    const CHUNK_BYTES: u64 = 1024 * 1024;
    const MAX_LOOKAHEAD: u64 = 2048;
    let path = path.as_ref();
    let known_scan = scan_image(path)?;
    let mut known_ranges = known_scan
        .partition_candidates
        .into_iter()
        .filter_map(|partition| {
            partition
                .offset_bytes
                .checked_add(partition.length_bytes)
                .map(|end| (partition.offset_bytes, end))
        })
        .collect::<Vec<_>>();
    known_ranges.sort_unstable();

    let image = ImageReader::open(path)?;
    let image_bytes = image.len();
    if image_bytes == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot search an empty image",
        ));
    }

    let mut candidates = Vec::new();
    let mut scanned_bytes = 0_u64;
    let mut known_range_index = 0_usize;
    let mut complete = true;
    while scanned_bytes < image_bytes {
        let segment_bytes = CHUNK_BYTES.min(image_bytes - scanned_bytes);
        let read_bytes = segment_bytes
            .saturating_add(MAX_LOOKAHEAD)
            .min(image_bytes - scanned_bytes);
        let read_length = usize::try_from(read_bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "scan chunk does not fit in memory",
            )
        })?;
        let mut buffer = vec![0_u8; read_length];
        image.read_exact_at(scanned_bytes, &mut buffer)?;

        let segment_length = usize::try_from(segment_bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "scan segment is too large")
        })?;
        let mut local_offset = 0_usize;
        while local_offset < segment_length && local_offset + 512 <= buffer.len() {
            let candidate_offset =
                scanned_bytes
                    .checked_add(local_offset as u64)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "scan offset overflows")
                    })?;
            while known_range_index < known_ranges.len()
                && known_ranges[known_range_index].1 <= candidate_offset
            {
                known_range_index += 1;
            }
            let in_known_partition = known_ranges
                .get(known_range_index)
                .is_some_and(|(start, end)| *start <= candidate_offset && candidate_offset < *end);
            let ntfs_signature = &buffer[local_offset + 3..local_offset + 11] == b"NTFS    ";
            let ext4_magic_offset = local_offset.checked_add(1080);
            let ext4_signature = ext4_magic_offset.is_some_and(|magic_offset| {
                buffer
                    .get(magic_offset..magic_offset + 2)
                    .is_some_and(|magic| magic == [0x53, 0xef])
            });

            if !in_known_partition && (ntfs_signature || ext4_signature) {
                let remaining_bytes = image_bytes - candidate_offset;
                if let Some(filesystem) =
                    identify_filesystem(&image, candidate_offset, remaining_bytes)?
                {
                    let Some(end) = candidate_offset.checked_add(filesystem.length_bytes) else {
                        local_offset += 512;
                        continue;
                    };
                    candidates.push(PartitionFinding {
                        table: "Lost scan",
                        index: u32::try_from(candidates.len() + 1).unwrap_or(u32::MAX),
                        type_label: "Filesystem start signature".to_owned(),
                        offset_bytes: candidate_offset,
                        length_bytes: filesystem.length_bytes,
                        filesystem: Some(filesystem),
                        issue: None,
                    });
                    known_ranges.push((candidate_offset, end));
                    known_ranges.sort_unstable();
                    known_range_index = 0;
                }
            }
            local_offset += 512;
        }

        scanned_bytes = scanned_bytes
            .checked_add(segment_bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "scan progress overflows"))?;
        if !report_progress(scanned_bytes, image_bytes) {
            complete = false;
            break;
        }
    }

    Ok(LostFilesystemScan {
        image_bytes,
        scanned_bytes,
        complete,
        candidates,
    })
}
