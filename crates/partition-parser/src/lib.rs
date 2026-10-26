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
    parse_gpt_at(reader, 1, false)
}

pub fn parse_backup_gpt<R: ReadAt>(reader: &R) -> io::Result<Gpt> {
    let image_sectors = reader.len() / SECTOR_SIZE as u64;
    let header_lba = image_sectors.checked_sub(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "image is too small for backup GPT",
        )
    })?;
    parse_gpt_at(reader, header_lba, true)
}

fn parse_gpt_at<R: ReadAt>(reader: &R, header_lba: u64, backup: bool) -> io::Result<Gpt> {
    let mut header = [0_u8; SECTOR_SIZE];
    let header_offset = header_lba
        .checked_mul(SECTOR_SIZE as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "GPT header offset overflows"))?;
    reader.read_exact_at(header_offset, &mut header)?;
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
    let expected_backup_lba = if backup {
        1
    } else {
        image_sectors.saturating_sub(1)
    };

    if current_lba != header_lba
        || backup_lba != expected_backup_lba
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
    let last_usable_end = last_usable_lba
        .checked_add(1)
        .and_then(|lba| lba.checked_mul(SECTOR_SIZE as u64))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "usable LBA overflows"))?;
    if (!backup && entries_end > first_usable_offset)
        || (backup && (entries_offset < last_usable_end || entries_end > header_offset))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GPT partition table overlaps a protected metadata range",
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

#[cfg(test)]
mod tests {
    use super::{Gpt, crc32, parse_backup_gpt};
    use disk_reader::ReadAt;
    use std::io;

    struct MemoryReader(Vec<u8>);

    impl ReadAt for MemoryReader {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
            let start = usize::try_from(offset)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "offset too large"))?;
            let end = start.checked_add(buffer.len()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "read range overflows")
            })?;
            let source = self
                .0
                .get(start..end)
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "short image"))?;
            buffer.copy_from_slice(source);
            Ok(())
        }
    }

    #[test]
    fn parses_backup_gpt_when_primary_header_is_damaged() {
        let mut image = vec![0_u8; 256 * 512];
        let mut entry = [0_u8; 128];
        entry[0] = 1;
        entry[16] = 2;
        entry[32..40].copy_from_slice(&34_u64.to_le_bytes());
        entry[40..48].copy_from_slice(&100_u64.to_le_bytes());
        let table_crc = crc32(&entry);
        image[223 * 512..223 * 512 + 128].copy_from_slice(&entry);

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
        header[88..92].copy_from_slice(&table_crc.to_le_bytes());
        let mut checksum_header = header[..92].to_vec();
        checksum_header[16..20].fill(0);
        let header_crc = crc32(&checksum_header);
        header[16..20].copy_from_slice(&header_crc.to_le_bytes());

        let gpt: Gpt = parse_backup_gpt(&MemoryReader(image)).unwrap();

        assert_eq!(gpt.current_lba, 255);
        assert_eq!(gpt.backup_lba, 1);
        assert_eq!(gpt.partitions.len(), 1);
        assert_eq!(gpt.partitions[0].first_lba, 34);
        assert_eq!(gpt.partitions[0].last_lba, 100);
    }
}
