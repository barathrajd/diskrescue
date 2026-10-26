#![forbid(unsafe_code)]

use std::io;

use disk_reader::ReadAt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSystemKind {
    Ntfs,
    Ext4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemCandidate {
    pub kind: FileSystemKind,
    pub offset_bytes: u64,
    pub length_bytes: u64,
    pub confidence: &'static str,
    pub reasons: Vec<&'static str>,
}

/// Identifies a supported filesystem at a partition boundary already found by
/// a partition parser. It reads only bounded boot-sector and superblock data.
pub fn identify_filesystem<R: ReadAt>(
    reader: &R,
    partition_offset_bytes: u64,
    partition_length_bytes: u64,
) -> io::Result<Option<FileSystemCandidate>> {
    let partition_end = partition_offset_bytes
        .checked_add(partition_length_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "partition range overflows"))?;
    if partition_length_bytes == 0 || partition_end > reader.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "partition range exceeds the image",
        ));
    }

    if let Some(candidate) = identify_ntfs(reader, partition_offset_bytes, partition_length_bytes)?
    {
        return Ok(Some(candidate));
    }
    identify_ext4(reader, partition_offset_bytes, partition_length_bytes)
}

fn identify_ntfs<R: ReadAt>(
    reader: &R,
    offset: u64,
    partition_length_bytes: u64,
) -> io::Result<Option<FileSystemCandidate>> {
    if partition_length_bytes < 512 {
        return Ok(None);
    }
    let mut boot_sector = [0_u8; 512];
    reader.read_exact_at(offset, &mut boot_sector)?;
    let Ok(parsed) = fs_ntfs::parse_boot_sector(&boot_sector, partition_length_bytes) else {
        return Ok(None);
    };

    Ok(Some(FileSystemCandidate {
        kind: FileSystemKind::Ntfs,
        offset_bytes: offset,
        length_bytes: parsed.volume_size_bytes,
        confidence: "high",
        reasons: vec![
            "NTFS OEM identifier is present",
            "sector, cluster, MFT, and file-record geometry is consistent",
            "volume bounds and boot-sector signature are valid",
        ],
    }))
}

fn identify_ext4<R: ReadAt>(
    reader: &R,
    offset: u64,
    partition_length_bytes: u64,
) -> io::Result<Option<FileSystemCandidate>> {
    const SUPERBLOCK_OFFSET: u64 = 1024;
    const SUPERBLOCK_SIZE: usize = 1024;
    const EXT4_MAGIC_OFFSET: usize = 0x38;
    if partition_length_bytes < SUPERBLOCK_OFFSET + SUPERBLOCK_SIZE as u64 {
        return Ok(None);
    }
    let superblock_offset = offset.checked_add(SUPERBLOCK_OFFSET).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "superblock offset overflows")
    })?;
    let mut superblock = [0_u8; SUPERBLOCK_SIZE];
    reader.read_exact_at(superblock_offset, &mut superblock)?;
    if superblock[EXT4_MAGIC_OFFSET..EXT4_MAGIC_OFFSET + 2] != [0x53, 0xef] {
        return Ok(None);
    }

    let inode_count = u32::from_le_bytes(superblock[0..4].try_into().unwrap());
    let blocks_low = u32::from_le_bytes(superblock[4..8].try_into().unwrap());
    let log_block_size = u32::from_le_bytes(superblock[24..28].try_into().unwrap());
    let blocks_per_group = u32::from_le_bytes(superblock[32..36].try_into().unwrap());
    let incompat_features = u32::from_le_bytes(superblock[0x60..0x64].try_into().unwrap());
    let blocks_high = u32::from_le_bytes(superblock[0x150..0x154].try_into().unwrap());
    if inode_count == 0
        || blocks_low == 0
        || blocks_per_group == 0
        || log_block_size > 6
        || incompat_features & 0x80 != 0
        || blocks_high != 0
    {
        return Ok(None);
    }
    let block_size = 1024_u64
        .checked_shl(log_block_size)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "ext4 block size overflows"))?;
    let volume_bytes = u64::from(blocks_low)
        .checked_mul(block_size)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "ext4 volume size overflows"))?;
    if volume_bytes > partition_length_bytes {
        return Ok(None);
    }

    Ok(Some(FileSystemCandidate {
        kind: FileSystemKind::Ext4,
        offset_bytes: offset,
        length_bytes: volume_bytes,
        confidence: "high",
        reasons: vec![
            "ext4 superblock magic is present",
            "inode, block-group, and block-size fields are consistent",
            "filesystem size fits inside the partition bounds",
        ],
    }))
}

#[cfg(test)]
mod tests {
    use super::{FileSystemKind, identify_filesystem};
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
            let end = start
                .checked_add(buffer.len())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "range overflows"))?;
            let source = self.0.get(start..end).ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "read is outside the image")
            })?;
            buffer.copy_from_slice(source);
            Ok(())
        }
    }

    #[test]
    fn identifies_a_structurally_valid_ntfs_volume_at_its_partition_start() {
        let mut image = vec![0_u8; 32 * 1024];
        let boot = &mut image[4096..4608];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[11..13].copy_from_slice(&512_u16.to_le_bytes());
        boot[13] = 8;
        boot[40..48].copy_from_slice(&48_u64.to_le_bytes());
        boot[48..56].copy_from_slice(&2_u64.to_le_bytes());
        boot[56..64].copy_from_slice(&3_u64.to_le_bytes());
        boot[64] = 1;
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);

        let candidate = identify_filesystem(&MemoryReader(image), 4096, 24 * 1024)
            .unwrap()
            .unwrap();

        assert_eq!(candidate.kind, FileSystemKind::Ntfs);
        assert_eq!(candidate.offset_bytes, 4096);
        assert_eq!(candidate.length_bytes, 48 * 512);
        assert_eq!(candidate.confidence, "high");
        assert_eq!(candidate.reasons.len(), 3);
    }

    #[test]
    fn rejects_ntfs_mft_cluster_references_outside_the_volume() {
        let mut image = vec![0_u8; 32 * 512];
        let boot = &mut image[..512];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[11..13].copy_from_slice(&512_u16.to_le_bytes());
        boot[13] = 8;
        boot[40..48].copy_from_slice(&32_u64.to_le_bytes());
        boot[48..56].copy_from_slice(&4_u64.to_le_bytes());
        boot[56..64].copy_from_slice(&3_u64.to_le_bytes());
        boot[64] = 1;
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);

        let candidate = identify_filesystem(&MemoryReader(image), 0, 32 * 512).unwrap();

        assert!(candidate.is_none());
    }

    #[test]
    fn rejects_a_partition_extent_outside_the_image() {
        let reader = MemoryReader(vec![0; 4096]);

        let error = identify_filesystem(&reader, 2048, 4096).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn identifies_ext4_from_a_bounded_superblock() {
        let mut image = vec![0_u8; 64 * 1024];
        let superblock = &mut image[1024..2048];
        superblock[0..4].copy_from_slice(&32_u32.to_le_bytes());
        superblock[4..8].copy_from_slice(&64_u32.to_le_bytes());
        superblock[24..28].copy_from_slice(&0_u32.to_le_bytes());
        superblock[32..36].copy_from_slice(&16_u32.to_le_bytes());
        superblock[0x38..0x3a].copy_from_slice(&[0x53, 0xef]);

        let candidate = identify_filesystem(&MemoryReader(image), 0, 64 * 1024)
            .unwrap()
            .unwrap();

        assert_eq!(candidate.kind, FileSystemKind::Ext4);
        assert_eq!(candidate.offset_bytes, 0);
        assert_eq!(candidate.length_bytes, 64 * 1024);
        assert_eq!(candidate.reasons.len(), 3);
    }

    #[test]
    fn does_not_claim_ext4_with_unsupported_64_bit_block_counts() {
        let mut image = vec![0_u8; 64 * 1024];
        let superblock = &mut image[1024..2048];
        superblock[0..4].copy_from_slice(&32_u32.to_le_bytes());
        superblock[4..8].copy_from_slice(&64_u32.to_le_bytes());
        superblock[24..28].copy_from_slice(&0_u32.to_le_bytes());
        superblock[32..36].copy_from_slice(&16_u32.to_le_bytes());
        superblock[0x38..0x3a].copy_from_slice(&[0x53, 0xef]);
        superblock[0x60..0x64].copy_from_slice(&0x80_u32.to_le_bytes());

        let candidate = identify_filesystem(&MemoryReader(image), 0, 64 * 1024).unwrap();

        assert!(candidate.is_none());
    }
}
