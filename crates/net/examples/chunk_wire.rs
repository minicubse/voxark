//! Compare actual on-wire snapshots and replica publication on one generated column.
use primal_net::{ChunkData, Replica, ServerMsg, decode, encode};
use std::{hint::black_box, time::Instant};
fn main() {
    println!(
        "modified_cells,representation,message_bytes,encode_mean_us,decode_generate_install_mean_us"
    );
    for n in [0usize, 1, 100, 1400, 2048, 4096] {
        for full in [false, true] {
            let data = if full {
                let mut cells = vec![0; 4096];
                cells[..n].fill(7);
                ChunkData::Full(cells)
            } else {
                ChunkData::Sparse((0..n).map(|i| (i as u16, 7)).collect())
            };
            let msg = ServerMsg::Chunk {
                chunk: [0, 7, 0],
                revision: 1,
                data,
            };
            let bytes = encode(&msg).unwrap();
            let start = Instant::now();
            for _ in 0..300 {
                black_box(encode(black_box(&msg)).unwrap());
            }
            let encode_us = start.elapsed().as_secs_f64() * 1e6 / 300.;
            let start = Instant::now();
            for _ in 0..300 {
                let mut r = Replica::new(20261120);
                r.apply(&decode(black_box(&bytes)).unwrap()).unwrap();
                black_box(r);
            }
            println!(
                "{n},{},{},{encode_us:.3},{:.3}",
                if full { "full" } else { "sparse" },
                bytes.len(),
                start.elapsed().as_secs_f64() * 1e6 / 300.
            );
        }
    }
}
