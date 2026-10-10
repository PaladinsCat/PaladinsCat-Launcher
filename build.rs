//! Embed the existing logo without maintaining a second authored icon asset.
use std::{error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=assets/paladinscat.png");
    if std::env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let png = include_bytes!("assets/paladinscat.png");
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || &png[12..16] != b"IHDR" {
        return Err("PaladinsCat icon must be a PNG with an IHDR".into());
    }
    let width = u32::from_be_bytes(png[16..20].try_into()?);
    let height = u32::from_be_bytes(png[20..24].try_into()?);
    if !(1..=256).contains(&width) || !(1..=256).contains(&height) {
        return Err("PaladinsCat icon dimensions must be within 1..=256".into());
    }
    // ICO supports a complete PNG payload; preserve its original pixels/alpha.
    let mut ico = vec![0, 0, 1, 0, 1, 0, width as u8, height as u8, 0, 0];
    ico.extend(1u16.to_le_bytes());
    ico.extend(32u16.to_le_bytes());
    ico.extend(u32::try_from(png.len())?.to_le_bytes());
    ico.extend(22u32.to_le_bytes());
    ico.extend(png);
    let icon_path = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR missing")?)
        .join("paladinscat.ico");
    fs::write(&icon_path, ico)?;
    winresource::WindowsResource::new()
        .set("ProductName", "PaladinsCat Launcher")
        .set_icon(icon_path.to_str().ok_or("Icon path must be UTF-8")?)
        .compile()?;
    Ok(())
}
