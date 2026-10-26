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

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::{NtfsBootSector, parse_boot_sector};

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
}
