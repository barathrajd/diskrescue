#![forbid(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

/// A bounded, read-only random-access source.
pub trait ReadAt: Send + Sync {
    fn len(&self) -> u64;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()>;
}

/// A file-backed image opened without write access.
pub struct ImageReader {
    file: Mutex<File>,
    length: u64,
}

impl ImageReader {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "image source must be a regular file",
            ));
        }

        Ok(Self {
            file: Mutex::new(file),
            length: metadata.len(),
        })
    }
}

impl ReadAt for ImageReader {
    fn len(&self) -> u64 {
        self.length
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let length = u64::try_from(buffer.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "read length does not fit in u64",
            )
        })?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "read range overflows"))?;
        if end > self.length {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "read extends beyond the image boundary",
            ));
        }

        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("image reader lock was poisoned"))?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buffer)
    }
}
