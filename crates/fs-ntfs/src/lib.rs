#![forbid(unsafe_code)]

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NtfsError {
    TruncatedBootSector,
    InvalidSignature,
    UnsupportedSectorSize,
    InvalidClusterGeometry,
    VolumeExceedsPartition,
    InvalidMftLocation,
    InvalidFileRecordSize,
    ArithmeticOverflow,
    InvalidFileRecord,
    InvalidUpdateSequenceArray,
    InvalidAttribute,
    InvalidFileName,
}

impl fmt::Display for NtfsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::TruncatedBootSector => "NTFS boot sector is truncated",
            Self::InvalidSignature => "NTFS boot-sector signature is invalid",
            Self::UnsupportedSectorSize => "NTFS sector size is unsupported",
            Self::InvalidClusterGeometry => "NTFS cluster geometry is invalid",
            Self::VolumeExceedsPartition => "NTFS volume exceeds its partition bounds",
            Self::InvalidMftLocation => "NTFS MFT location is outside the volume",
            Self::InvalidFileRecordSize => "NTFS file-record size is unsupported",
            Self::ArithmeticOverflow => "NTFS boot-sector arithmetic overflows",
            Self::InvalidFileRecord => "NTFS file record is invalid",
            Self::InvalidUpdateSequenceArray => "NTFS update-sequence array is invalid",
            Self::InvalidAttribute => "NTFS file-record attribute is invalid",
            Self::InvalidFileName => "NTFS file name is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for NtfsError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtfsBootSector {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub cluster_size_bytes: u32,
    pub total_sectors: u64,
    pub volume_size_bytes: u64,
    pub mft_cluster: u64,
    pub mft_mirror_cluster: u64,
    pub file_record_size_bytes: u32,
    pub volume_serial: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtfsFileRecord {
    pub record_number: u64,
    pub sequence_number: u16,
    pub in_use: bool,
    pub is_directory: bool,
    pub name: Option<String>,
    pub parent_record_number: Option<u64>,
    pub data_size_bytes: Option<u64>,
}

/// Parses a fixed-up NTFS FILE record and its resident metadata attributes.
pub fn parse_file_record(
    record: &[u8],
    record_number: u64,
    bytes_per_sector: u16,
) -> Result<NtfsFileRecord, NtfsError> {
    let sector = usize::from(bytes_per_sector);
    if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096)
        || record.len() < sector
        || record.len() % sector != 0
        || record.get(0..4) != Some(b"FILE")
    {
        return Err(NtfsError::InvalidFileRecord);
    }
    let usa_offset = usize::from(read_u16(record, 4));
    let usa_count = usize::from(read_u16(record, 6));
    let expected_count = record.len() / sector + 1;
    let usa_bytes = usa_count
        .checked_mul(2)
        .ok_or(NtfsError::ArithmeticOverflow)?;
    let usa_end = usa_offset
        .checked_add(usa_bytes)
        .ok_or(NtfsError::ArithmeticOverflow)?;
    if usa_count != expected_count || usa_offset < 8 || usa_end > record.len() {
        return Err(NtfsError::InvalidUpdateSequenceArray);
    }
    let sequence = read_u16(record, usa_offset);
    let mut fixed = record.to_vec();
    for index in 1..usa_count {
        let tail = index
            .checked_mul(sector)
            .and_then(|n| n.checked_sub(2))
            .ok_or(NtfsError::ArithmeticOverflow)?;
        let saved_offset = usa_offset
            .checked_add(index * 2)
            .ok_or(NtfsError::ArithmeticOverflow)?;
        if read_u16(record, tail) != sequence {
            return Err(NtfsError::InvalidUpdateSequenceArray);
        }
        fixed[tail..tail + 2].copy_from_slice(&record[saved_offset..saved_offset + 2]);
    }
    let first_attr = usize::from(read_u16(&fixed, 20));
    let flags = read_u16(&fixed, 22);
    let bytes_in_use =
        usize::try_from(read_u32(&fixed, 24)).map_err(|_| NtfsError::InvalidFileRecord)?;
    let bytes_allocated =
        usize::try_from(read_u32(&fixed, 28)).map_err(|_| NtfsError::InvalidFileRecord)?;
    if bytes_in_use > record.len()
        || bytes_allocated > record.len()
        || bytes_in_use < first_attr
        || first_attr < 32
    {
        return Err(NtfsError::InvalidFileRecord);
    }
    let mut result = NtfsFileRecord {
        record_number,
        sequence_number: read_u16(&fixed, 16),
        in_use: flags & 1 != 0,
        is_directory: flags & 2 != 0,
        name: None,
        parent_record_number: None,
        data_size_bytes: None,
    };
    let mut pos = first_attr;
    while pos
        .checked_add(4)
        .filter(|end| *end <= bytes_in_use)
        .ok_or(NtfsError::InvalidAttribute)?
        <= bytes_in_use
    {
        let kind = read_u32(&fixed, pos);
        if kind == u32::MAX {
            break;
        }
        let header_end = pos.checked_add(16).ok_or(NtfsError::ArithmeticOverflow)?;
        if header_end > bytes_in_use {
            return Err(NtfsError::InvalidAttribute);
        }
        let length =
            usize::try_from(read_u32(&fixed, pos + 4)).map_err(|_| NtfsError::InvalidAttribute)?;
        let end = pos
            .checked_add(length)
            .ok_or(NtfsError::ArithmeticOverflow)?;
        if length < 16 || end > bytes_in_use {
            return Err(NtfsError::InvalidAttribute);
        }
        let nonresident = fixed[pos + 8] != 0;
        let name_len = fixed[pos + 9];
        if kind == 0x30 {
            if nonresident || length < 24 {
                return Err(NtfsError::InvalidAttribute);
            }
            let value_len = usize::try_from(read_u32(&fixed, pos + 16))
                .map_err(|_| NtfsError::InvalidAttribute)?;
            let value_offset = usize::from(read_u16(&fixed, pos + 20));
            let value_start = pos
                .checked_add(value_offset)
                .ok_or(NtfsError::ArithmeticOverflow)?;
            let value_end = value_start
                .checked_add(value_len)
                .ok_or(NtfsError::ArithmeticOverflow)?;
            if value_len < 66 || value_end > end {
                return Err(NtfsError::InvalidFileName);
            }
            let value = &fixed[value_start..value_end];
            let chars = usize::from(value[64]);
            let name_bytes = chars.checked_mul(2).ok_or(NtfsError::ArithmeticOverflow)?;
            let name_end = 66_usize
                .checked_add(name_bytes)
                .ok_or(NtfsError::ArithmeticOverflow)?;
            if name_end > value.len() {
                return Err(NtfsError::InvalidFileName);
            }
            let units = value[66..name_end]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            let name = String::from_utf16(&units).map_err(|_| NtfsError::InvalidFileName)?;
            let parent = read_u64(value, 0) & 0x0000_ffff_ffff_ffff;
            if result.name.is_none() || (name_len == 0 && matches!(value[65], 1 | 3)) {
                result.name = Some(name);
                result.parent_record_number = Some(parent);
            }
        } else if kind == 0x80 && name_len == 0 {
            let size = if nonresident {
                if length < 56 {
                    return Err(NtfsError::InvalidAttribute);
                }
                read_u64(&fixed, pos + 48)
            } else {
                if length < 24 {
                    return Err(NtfsError::InvalidAttribute);
                }
                u64::from(read_u32(&fixed, pos + 16))
            };
            result.data_size_bytes = Some(size);
        }
        pos = end;
    }
    Ok(result)
}

/// Parses NTFS boot-sector geometry without reading beyond the first sector.
pub fn parse_boot_sector(
    boot_sector: &[u8],
    partition_length_bytes: u64,
) -> Result<NtfsBootSector, NtfsError> {
    if boot_sector.len() < 512 {
        return Err(NtfsError::TruncatedBootSector);
    }
    if &boot_sector[3..11] != b"NTFS    " || boot_sector[510..512] != [0x55, 0xaa] {
        return Err(NtfsError::InvalidSignature);
    }

    let bytes_per_sector = read_u16(boot_sector, 11);
    if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
        return Err(NtfsError::UnsupportedSectorSize);
    }
    let sectors_per_cluster = boot_sector[13];
    if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
        return Err(NtfsError::InvalidClusterGeometry);
    }
    let cluster_size_bytes = u32::from(bytes_per_sector)
        .checked_mul(u32::from(sectors_per_cluster))
        .filter(|size| *size <= 2 * 1024 * 1024)
        .ok_or(NtfsError::InvalidClusterGeometry)?;
    let total_sectors = read_u64(boot_sector, 40);
    let volume_size_bytes = total_sectors
        .checked_mul(u64::from(bytes_per_sector))
        .ok_or(NtfsError::ArithmeticOverflow)?;
    if total_sectors == 0 || volume_size_bytes > partition_length_bytes {
        return Err(NtfsError::VolumeExceedsPartition);
    }
    let cluster_count = total_sectors / u64::from(sectors_per_cluster);
    let mft_cluster = read_u64(boot_sector, 48);
    let mft_mirror_cluster = read_u64(boot_sector, 56);
    if mft_cluster >= cluster_count || mft_mirror_cluster >= cluster_count {
        return Err(NtfsError::InvalidMftLocation);
    }

    let file_record_size_bytes = match boot_sector[64] as i8 {
        code if code > 0 => cluster_size_bytes
            .checked_mul(code as u32)
            .ok_or(NtfsError::ArithmeticOverflow)?,
        code if code < 0 => {
            let shift = code.unsigned_abs();
            1_u32
                .checked_shl(u32::from(shift))
                .ok_or(NtfsError::InvalidFileRecordSize)?
        }
        _ => return Err(NtfsError::InvalidFileRecordSize),
    };
    if !(512..=1024 * 1024).contains(&file_record_size_bytes) {
        return Err(NtfsError::InvalidFileRecordSize);
    }

    Ok(NtfsBootSector {
        bytes_per_sector,
        sectors_per_cluster,
        cluster_size_bytes,
        total_sectors,
        volume_size_bytes,
        mft_cluster,
        mft_mirror_cluster,
        file_record_size_bytes,
        volume_serial: read_u64(boot_sector, 72),
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::{NtfsBootSector, parse_boot_sector, parse_file_record};

    #[test]
    fn parses_ntfs_geometry_and_mft_locations_from_a_valid_boot_sector() {
        let mut boot = [0_u8; 512];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[11..13].copy_from_slice(&512_u16.to_le_bytes());
        boot[13] = 8;
        boot[40..48].copy_from_slice(&4096_u64.to_le_bytes());
        boot[48..56].copy_from_slice(&16_u64.to_le_bytes());
        boot[56..64].copy_from_slice(&32_u64.to_le_bytes());
        boot[64] = 1;
        boot[72..80].copy_from_slice(&0x1234_5678_u64.to_le_bytes());
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);

        let parsed: NtfsBootSector = parse_boot_sector(&boot, 4096 * 512).unwrap();

        assert_eq!(parsed.bytes_per_sector, 512);
        assert_eq!(parsed.sectors_per_cluster, 8);
        assert_eq!(parsed.cluster_size_bytes, 4096);
        assert_eq!(parsed.volume_size_bytes, 4096 * 512);
        assert_eq!(parsed.mft_cluster, 16);
        assert_eq!(parsed.mft_mirror_cluster, 32);
        assert_eq!(parsed.file_record_size_bytes, 4096);
        assert_eq!(parsed.volume_serial, 0x1234_5678);
    }

    #[test]
    fn rejects_boot_sector_geometry_that_exceeds_the_partition() {
        let mut boot = [0_u8; 512];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[11..13].copy_from_slice(&512_u16.to_le_bytes());
        boot[13] = 8;
        boot[40..48].copy_from_slice(&4096_u64.to_le_bytes());
        boot[48..56].copy_from_slice(&16_u64.to_le_bytes());
        boot[56..64].copy_from_slice(&32_u64.to_le_bytes());
        boot[64] = 1;
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);

        assert!(parse_boot_sector(&boot, 512).is_err());
    }

    #[test]
    fn parses_deleted_file_name_and_parent_after_fixing_sector_tails() {
        let record = synthetic_file_record("photo.jpg", 5, false);

        let parsed = parse_file_record(&record, 42, 512).unwrap();

        assert_eq!(parsed.record_number, 42);
        assert_eq!(parsed.sequence_number, 7);
        assert!(!parsed.in_use);
        assert!(!parsed.is_directory);
        assert_eq!(parsed.name.as_deref(), Some("photo.jpg"));
        assert_eq!(parsed.parent_record_number, Some(5));
    }

    #[test]
    fn rejects_mft_record_with_mismatched_update_sequence_number() {
        let mut record = synthetic_file_record("photo.jpg", 5, false);
        record[510..512].copy_from_slice(&0x7777_u16.to_le_bytes());

        assert!(parse_file_record(&record, 42, 512).is_err());
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
}
