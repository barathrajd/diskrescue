#![forbid(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

const IMAGE_COPY_CHUNK_SIZE: usize = 1024 * 1024;

/// A bounded, read-only random-access source.
pub trait ReadAt: Send + Sync {
    fn len(&self) -> u64;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()>;
}

/// Copies a read-only source to a new image file and reports byte progress.
///
/// The caller must verify that the destination is on a different physical
/// device from the source. The source is never opened for writing here.
pub fn copy_to_image<R, F>(
    source: &R,
    destination: impl AsRef<Path>,
    report_progress: F,
) -> io::Result<u64>
where
    R: ReadAt,
    F: FnMut(u64, u64) -> bool,
{
    let destination = destination.as_ref();
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let available_bytes = fs2::available_space(parent)?;
    copy_to_image_with_space(source, destination, available_bytes, report_progress)
}

fn copy_to_image_with_space<R, F>(
    source: &R,
    destination: impl AsRef<Path>,
    available_bytes: u64,
    mut report_progress: F,
) -> io::Result<u64>
where
    R: ReadAt,
    F: FnMut(u64, u64) -> bool,
{
    let destination = destination.as_ref();
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let total_bytes = source.len();
    if total_bytes == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot create an image from an empty source",
        ));
    }
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "image destination already exists",
        ));
    }

    if available_bytes < total_bytes {
        return Err(io::Error::new(
            io::ErrorKind::StorageFull,
            "destination does not have enough free space for the image",
        ));
    }

    let mut image = tempfile::Builder::new()
        .prefix(".diskrescue-")
        .suffix(".partial")
        .tempfile_in(parent)?;
    let mut buffer = vec![0_u8; IMAGE_COPY_CHUNK_SIZE];
    let mut copied_bytes = 0_u64;

    while copied_bytes < total_bytes {
        let remaining = total_bytes - copied_bytes;
        let chunk_length = usize::try_from(remaining.min(IMAGE_COPY_CHUNK_SIZE as u64))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "image chunk is too large"))?;
        source.read_exact_at(copied_bytes, &mut buffer[..chunk_length])?;
        image.write_all(&buffer[..chunk_length])?;
        copied_bytes = copied_bytes
            .checked_add(chunk_length as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "image size overflows"))?;
        if !report_progress(copied_bytes, total_bytes) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "image creation was cancelled",
            ));
        }
    }

    image.as_file_mut().sync_all()?;
    image
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(copied_bytes)
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

#[cfg(test)]
mod tests {
    use super::{ImageReader, ReadAt, copy_to_image, copy_to_image_with_space};
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

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
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "range overflow"))?;
            let source = self.0.get(start..end).ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "read past memory source")
            })?;
            buffer.copy_from_slice(source);
            Ok(())
        }
    }

    #[test]
    fn copies_read_only_source_into_inspectable_image_and_reports_progress() {
        let directory = temporary_directory();
        let destination = directory.join("recovered-source.img");
        let source = MemoryReader(vec![0x5a; 1024 * 1024 + 19]);
        let mut progress = Vec::new();

        let copied = copy_to_image(&source, &destination, |copied, total| {
            progress.push((copied, total));
            true
        })
        .unwrap();

        assert_eq!(copied, source.len());
        assert_eq!(progress.last(), Some(&(copied, copied)));
        let image = ImageReader::open(&destination).unwrap();
        let mut first_byte = [0];
        image.read_exact_at(0, &mut first_byte).unwrap();
        assert_eq!(first_byte, [0x5a]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn refuses_to_replace_an_existing_image() {
        let directory = temporary_directory();
        let destination = directory.join("existing.img");
        fs::write(&destination, b"keep this file").unwrap();

        let error =
            copy_to_image(&MemoryReader(vec![0x33; 32]), &destination, |_, _| true).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&destination).unwrap(), b"keep this file");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn refuses_to_create_an_image_when_destination_space_is_insufficient() {
        let directory = temporary_directory();
        let destination = directory.join("too-large.img");
        let source = MemoryReader(vec![0x66; 128]);

        let error = copy_to_image_with_space(&source, &destination, 127, |_, _| true).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cancellation_does_not_leave_a_partial_image() {
        let directory = temporary_directory();
        let destination = directory.join("cancelled.img");
        let error =
            copy_to_image(&MemoryReader(vec![0x44; 64]), &destination, |_, _| false).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir_all(directory).unwrap();
    }

    fn temporary_directory() -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "diskrescue-copy-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
}
