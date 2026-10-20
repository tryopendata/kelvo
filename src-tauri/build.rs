use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

fn main() {
    agent_digests();
    tauri_build::build()
}

/// Compiles the SHA-256 of each Linux agent `make agent-linux` left in
/// `binaries/agent/` into `OUT_DIR/agent_digests.rs`, so the app re-hashes the bundled
/// resource before uploading it (`remote::agent_bundle`). Without the binaries the table
/// is empty and the app says to run `make agent-linux`.
fn agent_digests() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let out_dir = env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let dir = Path::new(&manifest_dir).join("binaries/agent");
    // A missing rerun-if-changed path reruns this script on every build, so the
    // (gitignored) directory is created empty instead. Cargo scans a watched directory's
    // entries, so binaries added, replaced or removed later all trigger a rerun.
    fs::create_dir_all(&dir).expect("create src-tauri/binaries/agent");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut entries: Vec<(String, String)> = Vec::new();
    for entry in fs::read_dir(&dir).expect("read src-tauri/binaries/agent") {
        let path = entry.expect("read src-tauri/binaries/agent entry").path();
        let Some(triple) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("kelvo-agent-"))
        else {
            continue;
        };
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(&path).expect("read bundled agent binary");
        entries.push((triple.to_owned(), hex_sha256(&bytes)));
    }
    entries.sort();

    let mut src = String::from("pub const AGENT_SHA256: &[(&str, &str)] = &[\n");
    for (triple, hex) in &entries {
        writeln!(src, "    ({triple:?}, {hex:?}),").expect("write to String");
    }
    src.push_str("];\n");
    fs::write(Path::new(&out_dir).join("agent_digests.rs"), src).expect("write agent_digests.rs");
}

fn hex_sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}
