#![forbid(unsafe_code)]

use disk_reader::{ImageReader, ReadAt};
use partition_hunter::{FileSystemCandidate, identify_filesystem};
use partition_parser::{Gpt, Mbr, parse_gpt, parse_mbr};
use std::io;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::{RecoveryScan, scan_image};
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
    let gpt = match &mbr {
        Ok(mbr) if mbr.protective => parse_gpt(&image).map_err(|error| error.to_string()),
        Ok(_) => Err("GPT not indicated by a protective MBR".to_owned()),
        Err(error) => Err(format!("GPT not checked: {error}")),
    };

    Ok(ImageInspection {
        image_bytes: image.len(),
        mbr,
        gpt,
    })
}

/// Scans partition-table entries in an image and checks each partition start
/// for supported NTFS or ext4 structures. This does not search unallocated
/// image regions for lost partition tables.
pub fn scan_image(path: impl AsRef<Path>) -> io::Result<RecoveryScan> {
    let image = ImageReader::open(path)?;
    let mbr = parse_mbr(&image);
    let gpt = match &mbr {
        Ok(mbr) if mbr.protective => parse_gpt(&image),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "GPT not indicated by a protective MBR",
        )),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("GPT not checked: {error}"),
        )),
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
                "GPT",
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
        gpt_error: gpt
            .err()
            .filter(|error| error.kind() != io::ErrorKind::NotFound)
            .map(|error| error.to_string()),
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
