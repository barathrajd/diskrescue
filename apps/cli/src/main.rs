#![forbid(unsafe_code)]

use recovery_core::{ImageInspection, inspect_image};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("diskrescue: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(usage)?;
    if command != "inspect" {
        return Err(usage());
    }

    let image_path = args.next().ok_or_else(usage)?;
    let json = match args.next() {
        None => false,
        Some(flag) if flag == "--json" => true,
        Some(_) => return Err(usage()),
    };
    if args.next().is_some() {
        return Err(usage());
    }

    let inspection = inspect_image(image_path).map_err(|error| error.to_string())?;
    if json {
        print_json(&inspection);
    } else {
        print_report(&inspection);
    }
    Ok(())
}

fn usage() -> String {
    "usage: diskrescue inspect <image-file> [--json]".to_owned()
}

fn print_report(inspection: &ImageInspection) {
    println!("DiskRescue image inspection (read-only)");
    println!("Image size: {} bytes", inspection.image_bytes);
    match &inspection.mbr {
        Ok(mbr) => {
            println!("MBR: valid, {} partition entries", mbr.partitions.len());
            for partition in &mbr.partitions {
                println!(
                    "  {}: type 0x{:02x}, start LBA {}, {} sectors",
                    partition.index, partition.type_code, partition.first_lba, partition.sectors
                );
            }
            if mbr.protective {
                println!("GPT: protective MBR found");
                match &inspection.gpt {
                    Ok(gpt) => {
                        println!("GPT: valid, {} partitions", gpt.partitions.len());
                        for partition in &gpt.partitions {
                            println!(
                                "  {}: LBA {}–{}",
                                partition.index, partition.first_lba, partition.last_lba
                            );
                        }
                    }
                    Err(error) => println!("GPT: unavailable ({error})"),
                }
            } else {
                println!("GPT: no protective MBR found");
            }
        }
        Err(error) => println!("MBR: unavailable ({error})"),
    }
}

fn print_json(inspection: &ImageInspection) {
    let (mbr_valid, protective, mbr_partitions, mbr_error) = match &inspection.mbr {
        Ok(mbr) => {
            let partitions = mbr
                .partitions
                .iter()
                .map(|partition| {
                    format!(
                        "{{\"index\":{},\"type_code\":\"0x{:02x}\",\"first_lba\":{},\"sectors\":{}}}",
                        partition.index, partition.type_code, partition.first_lba, partition.sectors
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            (
                "true",
                mbr.protective.to_string(),
                partitions,
                "null".to_owned(),
            )
        }
        Err(error) => (
            "false",
            "false".to_owned(),
            String::new(),
            json_string(error),
        ),
    };
    let (gpt_valid, gpt_partitions, gpt_error) = match &inspection.gpt {
        Ok(gpt) => {
            let partitions = gpt
                .partitions
                .iter()
                .map(|partition| {
                    let guid = partition
                        .type_guid
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>();
                    format!(
                        "{{\"index\":{},\"type_guid_bytes_hex\":\"{}\",\"first_lba\":{},\"last_lba\":{}}}",
                        partition.index, guid, partition.first_lba, partition.last_lba
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            ("true", partitions, "null".to_owned())
        }
        Err(error) => ("false", String::new(), json_string(error)),
    };

    println!(
        "{{\"read_only\":true,\"image_bytes\":{},\"mbr\":{{\"valid\":{},\"protective\":{},\"partitions\":[{}],\"error\":{}}},\"gpt\":{{\"valid\":{},\"partitions\":[{}],\"error\":{}}}}}",
        inspection.image_bytes,
        mbr_valid,
        protective,
        mbr_partitions,
        mbr_error,
        gpt_valid,
        gpt_partitions,
        gpt_error,
    );
}

fn json_string(value: &str) -> String {
    let escaped = value
        .chars()
        .flat_map(|character| match character {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            character if character.is_control() => "�".chars().collect(),
            character => vec![character],
        })
        .collect::<String>();
    format!("\"{escaped}\"")
}
