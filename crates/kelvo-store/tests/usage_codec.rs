#![allow(clippy::unwrap_used)]
//! The public reader rejects corrupt compressed facts instead of inventing readings.
mod common;
use common::*;
use kelvo_schema::durable_usage::*;
use kelvo_store::{StoreError, usage::*};
use std::{
    io::{Read, Write},
    sync::Arc,
};

fn fixture() -> (TempDir, kelvo_store::Store) {
    let dir = temp_dir("usage-codec");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let coverage = [UsageSourceCoverage {
        supported: true,
        measured_ms: 10_000,
        causes: 0,
    }; 5];
    let facts = UsageFacts {
        cpu_seconds: 0.125,
        gpu_seconds: 0.375,
        energy_j: 0.0625,
        mem_peak_bytes: 999,
        mem_byte_ms: 1_212_500.0,
        present_ms: 10_000,
        network_rx_bytes: 123,
        network_tx_bytes: 456,
        coverage,
    };
    w.write_usage_bucket(
        host(1).id,
        UsageBucket {
            from_ms: T0,
            to_ms: T0 + 10_000,
            source: coverage,
            apps: vec![UsageAppFacts {
                name: Arc::from("Editor"),
                facts,
            }],
            processes: vec![],
            uncertain_network: vec![],
        },
    )
    .unwrap();
    w.flush().unwrap();
    (dir, store)
}
fn query(store: &kelvo_store::Store) -> kelvo_store::Result<HistoricalUsageReply> {
    store.reader().unwrap().historical_usage_after(
        &HistoricalUsageRequest {
            host: host(1).id,
            from_ms: T0,
            to_ms: T0 + 10_000,
            by: HistoricalUsageMetric::Cpu,
            app_names: vec![],
        },
        Vec::new,
    )
}
#[test]
fn fractional_integrals_and_nonzero_facts_survive_restart_losslessly() {
    let (dir, store) = fixture();
    store.close().unwrap();
    let store = open(&dir, "h.sqlite");
    let reply = query(&store).unwrap();
    let f = &reply.apps[0].figures;
    assert_eq!(f.cpu_avg_pct, Some(1.25));
    assert_eq!(f.gpu_avg_pct, Some(3.75));
    assert_eq!(f.energy_j, Some(0.0625));
    assert_eq!(f.cpu_power_w, Some(0.00625));
    assert_eq!(f.mem_avg_bytes, Some(121));
    assert_eq!(f.mem_peak_bytes, Some(999));
    assert_eq!(f.network_rx_bytes, Some(123));
    assert_eq!(f.network_tx_bytes, Some(456));
}
#[test]
fn unsupported_truncated_oversized_and_invalid_integral_facts_return_corruption() {
    for defect in ["version", "truncated", "count", "integral"] {
        let (_dir, store) = fixture();
        let db = rusqlite::Connection::open(store.path()).unwrap();
        let mut blob: Vec<u8> = db
            .query_row("SELECT blob FROM usage_app_buckets", [], |r| r.get(0))
            .unwrap();
        if blob.first() == Some(&2) {
            blob = blob[(7 + 12 * usize::from(blob[1]))..].to_vec();
        }
        match defect {
            "version" => blob[0] = 255,
            "truncated" => blob.truncate(blob.len() / 2),
            "count" => blob[1..5].copy_from_slice(&1_000_001_u32.to_le_bytes()),
            _ => {
                let mut raw = vec![];
                flate2::read::ZlibDecoder::new(&blob[5..])
                    .read_to_end(&mut raw)
                    .unwrap();
                raw[8..16].copy_from_slice(&f64::NAN.to_le_bytes());
                let mut e =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(&raw).unwrap();
                blob.truncate(5);
                blob.extend(e.finish().unwrap());
            }
        }
        db.execute("UPDATE usage_app_buckets SET blob=?1", [blob])
            .unwrap();
        assert!(
            matches!(query(&store), Err(StoreError::Corrupt(_))),
            "{defect}"
        );
    }
}
