//! Build the 4B.2 acceptance world in a new directory. Never opens an existing save.
use primal_storage::RegionStore;
use primal_voxel::{Preset, chunk_at};
use std::{collections::BTreeMap, env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    let dir = PathBuf::from(
        env::args_os()
            .nth(1)
            .ok_or_else(|| io::Error::other("usage: navigation_wall <new-directory>"))?,
    );
    fs::create_dir(&dir)?;
    let path = dir.join("forest.pvx");
    let store = RegionStore::open_with_preset(&path, 20261120, Some(Preset::ForestModern))?;
    let mut chunks = BTreeMap::new();
    for z in 6..=9 {
        for y in 25..=26 {
            let pos = [-1, y, z];
            chunks
                .entry(chunk_at(pos))
                .or_insert_with(Vec::new)
                .push((pos, 3));
        }
    }
    for (key, edits) in chunks {
        store.save_chunk(key, &edits)?;
    }
    println!("{}", path.display());
    Ok(())
}
