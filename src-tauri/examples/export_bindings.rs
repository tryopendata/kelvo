//! Writes `src/core/generated/bindings.ts` without launching the app. Run by
//! `make bindings` and by CI's drift check.

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    kelvo_lib::export_bindings(&kelvo_lib::specta_builder())
        .with_context(|| format!("exporting bindings to {}", kelvo_lib::BINDINGS_PATH))?;
    println!("wrote {}", kelvo_lib::BINDINGS_PATH);
    Ok(())
}
