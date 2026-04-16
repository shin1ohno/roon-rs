use anyhow::Result;
use roon_api::{Core, ImageOptions};

pub async fn image(
    core: &Core,
    image_key: &str,
    width: Option<u32>,
    height: Option<u32>,
    scale: Option<&str>,
    format: Option<&str>,
    output_path: Option<&str>,
) -> Result<()> {
    let image_svc = core.image();
    let opts = ImageOptions {
        width,
        height,
        scale: scale.map(|s| s.to_string()),
        format: format.map(|f| {
            match f {
                "jpeg" | "jpg" => "image/jpeg".to_string(),
                "png" => "image/png".to_string(),
                other => other.to_string(),
            }
        }),
    };

    let bytes = image_svc.get_image(image_key, &opts).await?;

    match output_path {
        Some(path) => {
            std::fs::write(path, &bytes)?;
            println!("Saved {} bytes to {}", bytes.len(), path);
        }
        None => {
            // Write to stdout for piping
            use std::io::Write;
            std::io::stdout().write_all(&bytes)?;
        }
    }

    Ok(())
}
