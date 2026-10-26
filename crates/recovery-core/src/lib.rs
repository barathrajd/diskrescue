#![forbid(unsafe_code)]

use disk_reader::{ImageReader, ReadAt};
use partition_parser::{Gpt, Mbr, parse_gpt, parse_mbr};
use std::io;
use std::path::Path;

#[derive(Debug)]
pub struct ImageInspection {
    pub image_bytes: u64,
    pub mbr: Result<Mbr, String>,
    pub gpt: Result<Gpt, String>,
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
