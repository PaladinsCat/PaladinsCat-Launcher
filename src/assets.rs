//! Development-only asset preparation; the distributed GUI needs no source checkout.
use std::{
    fs,
    io::{Cursor, Read},
    path::PathBuf,
    time::Duration,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
    fs::create_dir_all(&dir)?;
    if !dir.join("Inter-Regular.ttf").exists() {
        let response = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(40))
            .build()
            .get("https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip")
            .call()?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(50 * 1024 * 1024)
            .read_to_end(&mut bytes)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        for (suffix, output) in [
            ("Inter-Regular.ttf", "Inter-Regular.ttf"),
            ("Inter-Bold.ttf", "Inter-Bold.ttf"),
            ("LICENSE.txt", "Inter-LICENSE.txt"),
        ] {
            let index = (0..archive.len())
                .find(|i| {
                    archive
                        .by_index(*i)
                        .is_ok_and(|f| f.name().ends_with(suffix))
                })
                .ok_or("Inter asset absent")?;
            let mut data = Vec::new();
            archive.by_index(index)?.read_to_end(&mut data)?;
            fs::write(dir.join(output), data)?;
        }
    }
    let logo = std::env::args()
        .nth(1)
        .ok_or("Pass the existing PaladinsCat PNG logo path")?;
    fs::copy(logo, dir.join("paladinscat.png"))?;
    println!(
        "Prepared embedded Inter Regular/Bold and original PaladinsCat logo in {}",
        dir.display()
    );
    Ok(())
}
