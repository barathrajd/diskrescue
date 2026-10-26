#![forbid(unsafe_code)]

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageReview {
    image_bytes: u64,
    mbr_status: String,
    mbr_partition_count: usize,
    gpt_status: String,
    gpt_partition_count: usize,
}

#[tauri::command]
fn inspect_image(path: String) -> Result<ImageReview, String> {
    let inspection = recovery_core::inspect_image(path).map_err(|error| error.to_string())?;
    let (mbr_status, mbr_partition_count) = match inspection.mbr {
        Ok(mbr) => ("Valid MBR".to_owned(), mbr.partitions.len()),
        Err(error) => (format!("Unreadable: {error}"), 0),
    };
    let (gpt_status, gpt_partition_count) = match inspection.gpt {
        Ok(gpt) => ("Valid GPT".to_owned(), gpt.partitions.len()),
        Err(error) if error == "GPT not indicated by a protective MBR" => {
            ("Not present".to_owned(), 0)
        }
        Err(error) => (format!("Unreadable: {error}"), 0),
    };

    Ok(ImageReview {
        image_bytes: inspection.image_bytes,
        mbr_status,
        mbr_partition_count,
        gpt_status,
        gpt_partition_count,
    })
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![inspect_image])
        .run(tauri::generate_context!())
        .expect("failed to run DiskRescue");
}

#[cfg(test)]
mod tests {
    use super::inspect_image;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn inspecting_selected_image_returns_partition_summary() {
        let path = temporary_image_path();
        let mut image = vec![0_u8; 512];
        image[510] = 0x55;
        image[511] = 0xaa;
        fs::write(&path, image).unwrap();

        let result = inspect_image(path.to_str().unwrap().to_owned()).unwrap();

        assert_eq!(result.image_bytes, 512);
        assert_eq!(result.mbr_partition_count, 0);
        assert_eq!(result.mbr_status, "Valid MBR");
        assert_eq!(result.gpt_status, "Not present");
        fs::remove_file(path).unwrap();
    }

    fn temporary_image_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("diskrescue-test-{nonce}.img"))
    }
}
