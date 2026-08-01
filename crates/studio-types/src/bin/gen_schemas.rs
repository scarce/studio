//! Regenerates `schemas/*.json` from the wire types (`just schemas`).
//! The drift test (`tests/schema_drift.rs`) fails CI when the checked-in
//! files are stale.

use std::path::Path;

fn main() -> std::io::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    for (name, schema) in studio_types::schemas::all() {
        let path = dir.join(format!("{name}.json"));
        let mut pretty = serde_json::to_string_pretty(&schema).expect("schema serializes");
        pretty.push('\n');
        std::fs::write(&path, pretty)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
