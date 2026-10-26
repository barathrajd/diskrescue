#![forbid(unsafe_code)]

use std::fmt;
use std::io;

use disk_reader::ReadAt;

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
    pub data_runs: Vec<(u64, u64)>,
}

/// Parses a bounded sequence of fixed-size MFT records and returns entries
/// whose in-use flag is clear. Non-FILE slots are treated as unused MFT space.
pub fn parse_deleted_file_records(
    bytes: &[u8],
    record_size_bytes: usize,
    bytes_per_sector: u16,
) -> Result<Vec<NtfsFileRecord>, NtfsError> {
    const MAX_WINDOW_BYTES: usize = 64 * 1024 * 1024;
    if !(512..=1024 * 1024).contains(&record_size_bytes)
        || !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096)
        || !record_size_bytes.is_multiple_of(usize::from(bytes_per_sector))
        || bytes.len() > MAX_WINDOW_BYTES
        || bytes.len() % record_size_bytes != 0
    {
        return Err(NtfsError::InvalidFileRecordSize);
    }
    let mut deleted = Vec::new();
    for (index, record) in bytes.chunks_exact(record_size_bytes).enumerate() {
        if record.get(0..4) != Some(b"FILE") {
            continue;
        }
        let record_number = u64::try_from(index).map_err(|_| NtfsError::ArithmeticOverflow)?;
        let parsed = parse_file_record(record, record_number, bytes_per_sector)?;
        if !parsed.in_use {
            deleted.push(parsed);
        }
    }
    Ok(deleted)
}

/// Reads record zero and its fragmented `$MFT` data stream from a bounded,
/// read-only image, then lists deleted records from that stream.
pub fn scan_deleted_file_records<R: ReadAt>(
    image: &R,
    partition_offset_bytes: u64,
    partition_length_bytes: u64,
) -> io::Result<Vec<NtfsFileRecord>> {
    const MAX_MFT_BYTES: u64 = 64 * 1024 * 1024;
    let partition_end = partition_offset_bytes
        .checked_add(partition_length_bytes)
        .filter(|end| *end <= image.len())
        .ok_or_else(|| invalid_image("NTFS partition exceeds the image"))?;
    let mut boot_bytes = [0_u8; 512];
    read_partition_bytes(
        image,
        partition_offset_bytes,
        partition_end,
        0,
        &mut boot_bytes,
    )?;
    let boot = parse_boot_sector(&boot_bytes, partition_length_bytes)
        .map_err(|error| invalid_image(error.to_string()))?;
    let cluster_size = u64::from(boot.cluster_size_bytes);
    let volume_clusters = boot.total_sectors / u64::from(boot.sectors_per_cluster);
    let mft_offset = boot
        .mft_cluster
        .checked_mul(cluster_size)
        .ok_or_else(|| invalid_image("MFT offset overflows"))?;
    let record_size = usize::try_from(boot.file_record_size_bytes)
        .map_err(|_| invalid_image("MFT record size is unsupported"))?;
    let mut base_record = vec![0_u8; record_size];
    read_partition_bytes(
        image,
        partition_offset_bytes,
        partition_end,
        mft_offset,
        &mut base_record,
    )?;
    let base = parse_file_record(&base_record, 0, boot.bytes_per_sector)
        .map_err(|error| invalid_image(error.to_string()))?;
    let stream_size = base
        .data_size_bytes
        .filter(|size| *size > 0 && *size <= MAX_MFT_BYTES && *size % record_size as u64 == 0)
        .ok_or_else(|| invalid_image("MFT data stream size is unsupported"))?;
    if base.data_runs.is_empty() {
        return Err(invalid_image("MFT data stream has no physical extents"));
    }

    let mut stream =
        vec![0_u8; usize::try_from(stream_size).map_err(|_| invalid_image("MFT is too large"))?];
    let mut stream_cursor = 0_usize;
    for (starting_cluster, cluster_count) in base.data_runs {
        if starting_cluster
            .checked_add(cluster_count)
            .filter(|end| *end <= volume_clusters)
            .is_none()
        {
            return Err(invalid_image("MFT extent exceeds the NTFS volume"));
        }
        let run_bytes = cluster_count
            .checked_mul(cluster_size)
            .ok_or_else(|| invalid_image("MFT extent size overflows"))?;
        let remaining = stream.len().saturating_sub(stream_cursor);
        let copy_bytes = usize::try_from(run_bytes.min(remaining as u64))
            .map_err(|_| invalid_image("MFT extent is too large"))?;
        if copy_bytes == 0 {
            break;
        }
        let relative_offset = starting_cluster
            .checked_mul(cluster_size)
            .ok_or_else(|| invalid_image("MFT extent offset overflows"))?;
        read_partition_bytes(
            image,
            partition_offset_bytes,
            partition_end,
            relative_offset,
            &mut stream[stream_cursor..stream_cursor + copy_bytes],
        )?;
        stream_cursor = stream_cursor
            .checked_add(copy_bytes)
            .ok_or_else(|| invalid_image("MFT stream offset overflows"))?;
    }
    if stream_cursor != stream.len() {
        return Err(invalid_image(
            "MFT extents do not cover the declared stream",
        ));
    }
    parse_deleted_file_records(&stream, record_size, boot.bytes_per_sector)
        .map_err(|error| invalid_image(error.to_string()))
}

fn read_partition_bytes<R: ReadAt>(
    image: &R,
    partition_start: u64,
    partition_end: u64,
    relative_offset: u64,
    buffer: &mut [u8],
) -> io::Result<()> {
    let absolute_offset = partition_start
        .checked_add(relative_offset)
        .ok_or_else(|| invalid_image("partition read offset overflows"))?;
    if absolute_offset
        .checked_add(buffer.len() as u64)
        .filter(|end| *end <= partition_end)
        .is_none()
    {
        return Err(invalid_image("partition read exceeds its bounds"));
    }
    image.read_exact_at(absolute_offset, buffer)
}

fn invalid_image(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Decodes a bounded NTFS nonresident data runlist into `(starting_cluster,
/// cluster_count)` extents. Sparse runs are rejected because this initial
/// reader does not yet map sparse streams.
pub fn parse_data_runlist(bytes: &[u8]) -> Result<Vec<(u64, u64)>, NtfsError> {
    let mut runs = Vec::new();
    let mut cursor = 0_usize;
    let mut current_lcn = 0_i64;
    loop {
        let header = *bytes.get(cursor).ok_or(NtfsError::InvalidAttribute)?;
        cursor = cursor.checked_add(1).ok_or(NtfsError::ArithmeticOverflow)?;
        if header == 0 {
            return Ok(runs);
        }
        let length_bytes = usize::from(header & 0x0f);
        let offset_bytes = usize::from(header >> 4);
        if length_bytes == 0 || length_bytes > 8 || offset_bytes == 0 || offset_bytes > 8 {
            return Err(NtfsError::InvalidAttribute);
        }
        let length_end = cursor
            .checked_add(length_bytes)
            .ok_or(NtfsError::ArithmeticOverflow)?;
        let offset_end = length_end
            .checked_add(offset_bytes)
            .ok_or(NtfsError::ArithmeticOverflow)?;
        if offset_end > bytes.len() {
            return Err(NtfsError::InvalidAttribute);
        }
        let count = decode_unsigned(&bytes[cursor..length_end])?;
        if count == 0 {
            return Err(NtfsError::InvalidAttribute);
        }
        let delta = decode_signed(&bytes[length_end..offset_end])?;
        current_lcn = current_lcn
            .checked_add(delta)
            .ok_or(NtfsError::ArithmeticOverflow)?;
        if current_lcn < 0 {
            return Err(NtfsError::InvalidAttribute);
        }
        runs.push((
            u64::try_from(current_lcn).map_err(|_| NtfsError::ArithmeticOverflow)?,
            count,
        ));
        cursor = offset_end;
    }
}

fn decode_unsigned(bytes: &[u8]) -> Result<u64, NtfsError> {
    let mut value = [0_u8; 8];
    value[..bytes.len()].copy_from_slice(bytes);
    Ok(u64::from_le_bytes(value))
}

fn decode_signed(bytes: &[u8]) -> Result<i64, NtfsError> {
    let fill = if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        0xff
    } else {
        0
    };
    let mut value = [fill; 8];
    value[..bytes.len()].copy_from_slice(bytes);
    Ok(i64::from_le_bytes(value))
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
        data_runs: Vec::new(),
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
                let runlist_offset = usize::from(read_u16(&fixed, pos + 32));
                if runlist_offset < 64 || runlist_offset >= length {
                    return Err(NtfsError::InvalidAttribute);
                }
                result.data_runs = parse_data_runlist(&fixed[pos + runlist_offset..end])?;
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
    use super::{NtfsBootSector, parse_boot_sector, parse_deleted_file_records, parse_file_record};

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
    fn parses_the_mft_data_runlist_from_a_nonresident_data_attribute() {
        let record = synthetic_mft_base_record();

        let parsed = parse_file_record(&record, 0, 512).unwrap();

        assert_eq!(parsed.data_size_bytes, Some(1024));
        assert_eq!(parsed.data_runs, vec![(5, 1), (12, 2)]);
    }

    #[test]
    fn reads_deleted_entries_from_a_fragmented_mft_in_an_image() {
        let image = synthetic_ntfs_image_with_fragmented_mft();
        let reader = MemoryReader(image);

        let deleted = super::scan_deleted_file_records(&reader, 0, 16 * 512).unwrap();

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].record_number, 2);
        assert_eq!(deleted[0].name.as_deref(), Some("lost.mp4"));
    }

    #[test]
    fn rejects_mft_runs_that_point_outside_the_ntfs_volume() {
        let mut image = synthetic_ntfs_image_with_fragmented_mft();
        image[2 * 512 + 56 + 64..2 * 512 + 56 + 71].copy_from_slice(&[0x11, 2, 250, 0x11, 6, 8, 0]);
        let reader = MemoryReader(image);

        assert!(super::scan_deleted_file_records(&reader, 0, 16 * 512).is_err());
    }

    #[test]
    fn lists_only_deleted_file_records_from_a_bounded_mft_window() {
        let mut mft = Vec::new();
        mft.extend_from_slice(&synthetic_file_record("live.jpg", 5, true));
        mft.extend_from_slice(&synthetic_file_record("lost.mp4", 5, false));
        mft.extend_from_slice(&[0_u8; 1024]);

        let entries = parse_deleted_file_records(&mft, 1024, 512).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].record_number, 1);
        assert_eq!(entries[0].name.as_deref(), Some("lost.mp4"));
    }

    #[test]
    fn rejects_partial_or_invalid_mft_record_windows() {
        assert!(parse_deleted_file_records(&[0; 1025], 1024, 512).is_err());
        assert!(parse_deleted_file_records(&[0; 1024], 513, 512).is_err());
    }

    #[test]
    fn rejects_mft_record_with_mismatched_update_sequence_number() {
        let mut record = synthetic_file_record("photo.jpg", 5, false);
        record[510..512].copy_from_slice(&0x7777_u16.to_le_bytes());

        assert!(parse_file_record(&record, 42, 512).is_err());
    }

    #[test]
    fn parses_signed_ntfs_data_runlist_extents() {
        let runs = super::parse_data_runlist(&[0x11, 3, 5, 0x11, 2, 4, 0]).unwrap();
        assert_eq!(runs, vec![(5, 3), (9, 2)]);
    }

    #[test]
    fn rejects_ntfs_data_runlist_with_truncated_fields() {
        assert!(super::parse_data_runlist(&[0x11, 3]).is_err());
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
        record[22..24].copy_from_slice(&1_u16.to_le_bytes());
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

    fn synthetic_mft_base_record() -> [u8; 1024] {
        let mut record = [0_u8; 1024];
        record[0..4].copy_from_slice(b"FILE");
        record[4..6].copy_from_slice(&48_u16.to_le_bytes());
        record[6..8].copy_from_slice(&3_u16.to_le_bytes());
        record[48..50].copy_from_slice(&0xaaaa_u16.to_le_bytes());
        record[50..52].copy_from_slice(&0x1111_u16.to_le_bytes());
        record[52..54].copy_from_slice(&0x2222_u16.to_le_bytes());
        record[20..22].copy_from_slice(&56_u16.to_le_bytes());
        record[22..24].copy_from_slice(&1_u16.to_le_bytes());
        record[24..28].copy_from_slice(&140_u32.to_le_bytes());
        record[28..32].copy_from_slice(&1024_u32.to_le_bytes());
        let attribute = &mut record[56..136];
        attribute[0..4].copy_from_slice(&0x80_u32.to_le_bytes());
        attribute[4..8].copy_from_slice(&80_u32.to_le_bytes());
        attribute[8] = 1;
        attribute[32..34].copy_from_slice(&64_u16.to_le_bytes());
        attribute[48..56].copy_from_slice(&1024_u64.to_le_bytes());
        attribute[64..70].copy_from_slice(&[0x11, 1, 5, 0x11, 2, 7]);
        attribute[70] = 0;
        record[136..140].copy_from_slice(&u32::MAX.to_le_bytes());
        record[510..512].copy_from_slice(&0xaaaa_u16.to_le_bytes());
        record[1022..1024].copy_from_slice(&0xaaaa_u16.to_le_bytes());
        record
    }

    struct MemoryReader(Vec<u8>);

    impl disk_reader::ReadAt for MemoryReader {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
            let start = usize::try_from(offset).unwrap();
            let end = start.checked_add(buffer.len()).unwrap();
            if end > self.0.len() {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            buffer.copy_from_slice(&self.0[start..end]);
            Ok(())
        }
    }

    fn synthetic_ntfs_image_with_fragmented_mft() -> Vec<u8> {
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

        let mut base = synthetic_mft_base_record();
        base[24..28].copy_from_slice(&140_u32.to_le_bytes());
        base[56 + 48..56 + 56].copy_from_slice(&4096_u64.to_le_bytes());
        base[56 + 64..56 + 71].copy_from_slice(&[0x11, 2, 2, 0x11, 6, 8, 0]);
        image[2 * 512..2 * 512 + 1024].copy_from_slice(&base);
        let live = synthetic_file_record("live.jpg", 5, true);
        let deleted = synthetic_file_record("lost.mp4", 5, false);
        image[10 * 512..10 * 512 + 1024].copy_from_slice(&live);
        image[12 * 512..12 * 512 + 1024].copy_from_slice(&deleted);
        image
    }
}
