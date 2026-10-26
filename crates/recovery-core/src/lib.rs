#![forbid(unsafe_code)]

use disk_reader::{ImageReader, ReadAt};
use partition_hunter::{FileSystemCandidate, identify_filesystem};
use partition_parser::{Gpt, Mbr, parse_backup_gpt, parse_gpt, parse_mbr};
use std::io;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::{RecoveryScan, inspect_image, scan_image};
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
