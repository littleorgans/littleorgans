use std::path::Path;
use std::{fs, io};

#[path = "src/tool_sources.rs"]
mod tool_sources;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../app/tools");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let app_root = Path::new(&manifest_dir).join("../app");
    let tools_dir = app_root.join("tools");
    let paths = tool_sources::ordered_tool_source_paths(&tools_dir).map_err(io::Error::other)?;
    for path in &paths {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let generated =
        tool_sources::render_tool_source_includes(&app_root, &paths).map_err(io::Error::other)?;
    let out_dir = std::env::var("OUT_DIR")?;
    fs::write(Path::new(&out_dir).join("tool_sources.rs"), generated)?;
    Ok(())
}
