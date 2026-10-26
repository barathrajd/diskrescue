#![forbid(unsafe_code)]

use disk_reader::ReadAt;
use std::io;

const SECTOR_SIZE: usize = 512;
const MBR_TABLE_OFFSET: usize = 446;
const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const MAX_GPT_ENTRIES: u32 = 4_096;
const MAX_GPT_ENTRY_SIZE: u32 = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbrPartition {
    pub index: u8,
    pub type_code: u8,
    pub first_lba: u32,
    pub sectors: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mbr {
    pub partitions: Vec<MbrPartition>,
    pub protective: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptPartition {
    pub index: u32,
    pub first_lba: u64,
    pub last_lba: u64,
    pub type_guid: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpt {
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub partitions: Vec<GptPartition>,
}

pub fn parse_mbr<R: ReadAt>(reader: &R) -> io::Result<Mbr> {
    let mut sector = [0_u8; SECTOR_SIZE];
    reader.read_exact_at(0, &mut sector)?;
    if sector[510..512] != [0x55, 0xaa] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing MBR signature",
        ));
    }

    let mut partitions = Vec::new();
    for slot in 0..4 {
        let start = MBR_TABLE_OFFSET + slot * 16;
        let entry = &sector[start..start + 16];
        let type_code = entry[4];
        let first_lba = u32::from_le_bytes(entry[8..12].try_into().unwrap());
        let sectors = u32::from_le_bytes(entry[12..16].try_into().unwrap());
        if type_code != 0 && sectors != 0 {
            let end_lba = u64::from(first_lba)
                .checked_add(u64::from(sectors))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "MBR partition range overflows")
                })?;
            if end_lba > reader.len() / SECTOR_SIZE as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "MBR partition extends beyond the image",
                ));
            }
            partitions.push(MbrPartition {
                index: (slot + 1) as u8,
                type_code,
                first_lba,
                sectors,
            });
        }
    }

    Ok(Mbr {
        protective: partitions
            .iter()
            .any(|partition| partition.type_code == 0xee),
        partitions,
    })
}

pub fn parse_gpt<R: ReadAt>(reader: &R) -> io::Result<Gpt> {
    let mut header = [0_u8; SECTOR_SIZE];
    reader.read_exact_at(SECTOR_SIZE as u64, &mut header)?;
    if &header[0..8] != GPT_SIGNATURE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing GPT header signature",
        ));
    }

    let header_size = u32::from_le_bytes(header[12..16].try_into().unwrap());
    if !(92..=SECTOR_SIZE as u32).contains(&header_size) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid GPT header size",
        ));
    }
    let expected_header_crc = u32::from_le_bytes(header[16..20].try_into().unwrap());
    let mut header_for_crc = header[..header_size as usize].to_vec();
    header_for_crc[16..20].fill(0);
    if crc32(&header_for_crc) != expected_header_crc {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GPT header CRC mismatch",
        ));
    }

    let current_lba = read_u64(&header, 24);
    let backup_lba = read_u64(&header, 32);
    let first_usable_lba = read_u64(&header, 40);
    let last_usable_lba = read_u64(&header, 48);
    let entries_lba = read_u64(&header, 72);
    let entry_count = read_u32(&header, 80);
    let entry_size = read_u32(&header, 84);
    let expected_entries_crc = read_u32(&header, 88);
    let image_sectors = reader.len() / SECTOR_SIZE as u64;

    if current_lba != 1
        || backup_lba == current_lba
        || backup_lba >= image_sectors
        || first_usable_lba > last_usable_lba
        || last_usable_lba >= image_sectors
        || entries_lba < 2
        || entry_count > MAX_GPT_ENTRIES
        || !(128..=MAX_GPT_ENTRY_SIZE).contains(&entry_size)
        || entry_size % 8 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GPT header fields are outside supported bounds",
        ));
    }

    let entries_bytes = u64::from(entry_count)
        .checked_mul(u64::from(entry_size))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "GPT table size overflows"))?;
    let entries_offset = entries_lba
        .checked_mul(SECTOR_SIZE as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "GPT offset overflows"))?;
    let entries_end = entries_offset
        .checked_add(entries_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "GPT table range overflows"))?;
    if entries_end > reader.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GPT partition table extends beyond the image",
        ));
    }
    let first_usable_offset = first_usable_lba
        .checked_mul(SECTOR_SIZE as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "usable LBA overflows"))?;
    if entries_end > first_usable_offset {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GPT partition table overlaps the usable LBA range",
        ));
    }

    let table_length = usize::try_from(entries_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "GPT table is too large"))?;
    let mut table = vec![0_u8; table_length];
    reader.read_exact_at(entries_offset, &mut table)?;
    if crc32(&table) != expected_entries_crc {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GPT partition table CRC mismatch",
        ));
    }

    let mut partitions = Vec::new();
    for index in 0..entry_count {
        let start = (index * entry_size) as usize;
        let entry = &table[start..start + entry_size as usize];
        if entry[..16].iter().all(|byte| *byte == 0) {
            continue;
        }
        let first_lba = read_u64(entry, 32);
        let last_lba = read_u64(entry, 40);
        if first_lba > last_lba || first_lba < first_usable_lba || last_lba > last_usable_lba {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "GPT partition lies outside the usable LBA range",
            ));
        }
        let mut type_guid = [0_u8; 16];
        type_guid.copy_from_slice(&entry[..16]);
        partitions.push(GptPartition {
            index: index + 1,
            first_lba,
            last_lba,
            type_guid,
        });
    }

    Ok(Gpt {
        current_lba,
        backup_lba,
        first_usable_lba,
        last_usable_lba,
        partitions,
    })
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn crc32(bytes: &[u8]) -> u32 {
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
