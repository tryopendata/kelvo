#![cfg(feature = "write-stats")]
#![allow(clippy::unwrap_used, clippy::too_many_lines)]
//! Ignored production storage fixture. No timing claims: suitable outside quiet CPU runs.
mod common;
use common::*;
use kelvo_schema::durable_usage::UsageIncompleteCause;
use kelvo_store::usage::*;
use std::sync::Arc;

#[test]
#[ignore = "production hourly SQLite volume fixture"]
fn production_hourly_volume() {
    let dir = temp_dir("usage-production-volume");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    w.flush().unwrap();
    let start = w.write_stats().unwrap();
    let names: Vec<Arc<str>> = (0..350)
        .map(|i| Arc::from(format!("Application {i:03}")))
        .collect();
    let mut generation = vec![0_i64; 1000];
    for tick in 0..360_i64 {
        if tick % 3 == 0 && tick > 0 {
            for offset in 0..50 {
                generation[((tick / 3 * 50 + offset) % 1000) as usize] += 1;
            }
        }
        let at = T0 + tick * 10_000;
        let mut source = [UsageSourceCoverage {
            supported: true,
            measured_ms: 10_000,
            causes: 0,
        }; 5];
        if tick % 31 == 0 {
            source[4].causes = cause_mask(UsageIncompleteCause::UnresolvedIdentity);
        }
        let mut b = UsageBucket {
            from_ms: at,
            to_ms: at + 10_000,
            source,
            apps: Vec::new(),
            processes: Vec::new(),
            uncertain_network: Vec::new(),
        };
        let mut app_facts = vec![UsageFacts::default(); 350];
        for (i, g) in generation.iter().enumerate() {
            let mut coverage = source;
            if i % 29 == 0 {
                coverage[1].measured_ms = 8000;
                coverage[1].causes |= cause_mask(UsageIncompleteCause::ReadFailed);
            }
            let cpu = if i % 20 == 0 {
                0.01 * (1 + (tick / 3 + i as i64) % 17) as f64
            } else {
                0.0
            };
            let mem = (16 + i as u64 % 64) * 1_048_576;
            let f = UsageFacts {
                cpu_seconds: cpu,
                gpu_seconds: cpu / 3.0,
                energy_j: cpu / 2.0,
                mem_peak_bytes: mem,
                mem_byte_ms: mem as f64 * 10_000.0,
                present_ms: 10_000,
                network_rx_bytes: if i % 20 == 0 { 1024 } else { 0 },
                network_tx_bytes: 0,
                coverage,
            };
            app_facts[i % 350].merge(&f);
            b.processes.push(UsageProcessFacts {
                app_name: names[i % 350].clone(),
                pid: 100 + i as i32,
                start_time_us: T0 * 1000 + g * 1_000_000 + i as i64,
                name: Arc::from(format!("worker-{i:04}")),
                facts: f,
            });
        }
        for (i, mut f) in app_facts.into_iter().enumerate() {
            // Fixture members run concurrently: app RAM is the concurrent sum, not max(process).
            f.mem_peak_bytes = (i..1000)
                .step_by(350)
                .map(|p| (16 + p as u64 % 64) * 1_048_576)
                .sum();
            f.present_ms = 10_000;
            for c in &mut f.coverage {
                c.measured_ms = c.measured_ms.min(10_000);
            }
            b.apps.push(UsageAppFacts {
                name: names[i].clone(),
                facts: f,
            });
        }
        if tick % 60 == 0 {
            b.uncertain_network
                .push(kelvo_schema::durable_usage::UsageNetworkUncertainty {
                    app_name: Some(names[0].to_string()),
                    possible_from_ms: at - 30_000,
                    possible_to_ms: at + 10_000,
                    rx_bytes: 1024,
                    tx_bytes: 128,
                    records: 1,
                });
        }
        w.write_usage_bucket(host(1).id, b).unwrap();
        if tick % 30 == 29 {
            w.flush().unwrap();
        }
    }
    w.flush().unwrap();
    let end = w.write_stats().unwrap();
    let db = raw(&store);
    let mut total_rows = 0;
    let mut payload = 0;
    for table in [
        "usage_apps",
        "usage_processes",
        "usage_app_buckets",
        "usage_process_buckets",
        "usage_coverage",
        "usage_uncertain",
        "usage_recording_start",
    ] {
        let columns: Vec<(String, String)> = db
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| Ok((r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let expression = columns
            .iter()
            .map(|(name, ty)| {
                if ty == "INTEGER" {
                    format!("CASE WHEN {name} IS NULL THEN 0 ELSE 8 END")
                } else {
                    format!("coalesce(length({name}),0)")
                }
            })
            .collect::<Vec<_>>()
            .join("+");
        let (rows, bytes): (i64, i64) = db
            .query_row(
                &format!("SELECT count(*),coalesce(sum({expression}),0) FROM {table}"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        println!("[production] {table}: {rows} rows {bytes} logical payload bytes");
        total_rows += rows as u64;
        payload += bytes as u64;
    }
    let wal = end.wal_bytes() - start.wal_bytes();
    println!(
        "[production] usage hourly rows={total_rows} payload={payload} WAL={wal}; combined baseline rows={} payload={} WAL={}",
        total_rows + 936,
        payload + 769392,
        wal + 4331712
    );
    assert!(total_rows + 936 <= 10_000, "combined row budget");
    assert!(payload + 769392 <= 4_000_000, "combined payload budget");
    assert!(wal + 4331712 <= 12_000_000, "combined WAL budget");
}

// Compile the production codec unchanged, without exposing persistence internals as API.
#[allow(dead_code)]
#[path = "../src/usage/codec.rs"]
mod codec;
use kelvo_store::StoreError;
type Result<T, E = StoreError> = std::result::Result<T, E>;

#[test]
#[ignore = "production codec/schema 30-day retained fill; no CPU timing claims"]
fn production_retained_fill() {
    let dir = temp_dir("usage-production-retained");
    let path = std::env::var_os("KELVO_USAGE_FILL_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.path().join("retained.sqlite"));
    let store = kelvo_store::Store::open(kelvo_store::StoreConfig::new(&path)).unwrap();
    store.writer().upsert_host(host(1)).unwrap();
    store.close().unwrap();
    let mut db = rusqlite::Connection::open(&path).unwrap();
    let horizon = 30 * DAY;
    let count = 1000 + ((horizon - 1) / 30_000) * 50;
    let tx = db.transaction().unwrap();
    tx.execute(
        "INSERT INTO usage_recording_start(host_id,from_ms) VALUES(1,?1)",
        [T0],
    )
    .unwrap();
    {
        let mut app = tx
            .prepare("INSERT INTO usage_apps(id,host_id,name,last_ms) VALUES(?1,1,?2,?3)")
            .unwrap();
        for i in 0..350 {
            app.execute(rusqlite::params![
                i + 1,
                format!("Application {i:03}"),
                T0 + horizon
            ])
            .unwrap();
        }
        let mut proc=tx.prepare("INSERT INTO usage_processes(id,host_id,app_id,pid,start_time_us,name,last_ms) VALUES(?1,1,?2,?3,?4,?5,?6)").unwrap();
        for id in 0..count {
            let (slot, born) = if id < 1000 {
                (id, 0)
            } else {
                ((id - 1000) % 1000, ((id - 1000) / 50 + 1) * 30_000)
            };
            proc.execute(rusqlite::params![
                id + 1,
                slot % 350 + 1,
                slot + 100,
                (T0 + born) * 1000,
                format!("worker-{slot:04}"),
                T0 + (born + 600_000).min(horizon)
            ])
            .unwrap();
        }
    }
    tx.commit().unwrap();
    let mut start = 0;
    let mut buckets = 0;
    while start < horizon {
        // Oldest 23 days at 15m, following six at 1m, finest final day at 10s.
        let width = if start < 23 * DAY {
            900_000
        } else if start < 29 * DAY {
            60_000
        } else {
            10_000
        };
        let tx = db.transaction().unwrap();
        let mut apps = std::collections::BTreeMap::<u64, UsageFacts>::new();
        let mut processes = std::collections::BTreeMap::<u64, UsageFacts>::new();
        let mut source = [UsageSourceCoverage::default(); 5];
        for at in (start..start + width).step_by(10_000) {
            let mut concurrent = vec![0_u64; 350];
            for slot in 0..1000_i64 {
                let event = at / 30_000;
                let cycles = event / 20;
                let replaced = (slot / 50) < event % 20;
                let id = if cycles == 0 && !replaced {
                    slot + 1
                } else {
                    1001 + ((cycles - i64::from(!replaced)) * 20 + slot / 50) * 50 + slot % 50
                };
                let cpu = if slot % 20 == 0 {
                    0.01 * (1 + (at / 30_000 + slot) % 17) as f64
                } else {
                    0.0
                };
                let mem = (16 + slot as u64 % 64) * 1_048_576;
                let mut cov = [UsageSourceCoverage {
                    supported: true,
                    measured_ms: 10_000,
                    causes: 0,
                }; 5];
                if slot % 29 == 0 {
                    cov[1].measured_ms = 8000;
                    cov[1].causes = cause_mask(UsageIncompleteCause::ReadFailed);
                }
                let f = UsageFacts {
                    cpu_seconds: cpu,
                    gpu_seconds: cpu / 3.0,
                    energy_j: cpu / 2.0,
                    mem_peak_bytes: mem,
                    mem_byte_ms: mem as f64 * 10_000.0,
                    present_ms: 10_000,
                    network_rx_bytes: if slot % 20 == 0 { 1024 } else { 0 },
                    network_tx_bytes: 0,
                    coverage: cov,
                };
                processes.entry(id as u64).or_default().merge(&f);
                apps.entry((slot % 350 + 1) as u64).or_default().merge(&f);
                concurrent[(slot % 350) as usize] += mem;
            }
            for (i, mem) in concurrent.into_iter().enumerate() {
                apps.get_mut(&(i as u64 + 1)).unwrap().mem_peak_bytes =
                    apps[&(i as u64 + 1)].mem_peak_bytes.max(mem);
            }
            for c in &mut source {
                c.supported = true;
                c.measured_ms += 10_000;
            }
        }
        for f in apps.values_mut() {
            f.present_ms = width as u64;
            for c in &mut f.coverage {
                c.measured_ms = c.measured_ms.min(width as u64);
            }
        }
        for (table, rows) in [
            ("usage_app_buckets", apps.into_iter().collect::<Vec<_>>()),
            ("usage_process_buckets", processes.into_iter().collect()),
            (
                "usage_coverage",
                vec![(
                    0,
                    UsageFacts {
                        coverage: source,
                        ..UsageFacts::default()
                    },
                )],
            ),
        ] {
            let blob = codec::encode(&rows).unwrap();
            tx.execute(
                &format!(
                    "INSERT INTO {table}(host_id,width_ms,bucket_ts,seq,blob) VALUES(1,?1,?2,?3,?4)"
                ),
                rusqlite::params![width, T0 + start, buckets + 1, blob],
            )
            .unwrap();
        }
        if start % 600_000 == 0 {
            tx.execute("INSERT INTO usage_uncertain(host_id,app_id,discovery_bucket_ts,possible_from_ms,possible_to_ms,rx_bytes,tx_bytes,records,seq) VALUES(1,1,?1,?2,?3,1024,128,1,?4)",rusqlite::params![T0+start,T0+start-30_000,T0+start+10_000,buckets+1]).unwrap();
        }
        tx.commit().unwrap();
        start += width;
        buckets += 1;
        if buckets % 2000 == 0 {
            println!("[production retained] {buckets} buckets seeded");
        }
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    let bytes:Vec<(String,i64)>=db.prepare("SELECT coalesce(s.tbl_name,d.name),sum(d.pgsize) FROM dbstat d LEFT JOIN sqlite_schema s ON s.name=d.name GROUP BY 1").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?))).unwrap().collect::<Result<_,_>>().unwrap();
    println!(
        "[production retained] {count} identities {buckets} retained buckets SQLite={size} bytes path={}",
        path.display()
    );
    for (table, n) in bytes {
        println!("[production retained] {table} {n} bytes incl indexes");
    }
    assert!(
        size + 150_000_000 <= 750_000_000,
        "usage plus unchanged existing 150 MB scope exceeds approved default"
    );
}

#[test]
#[ignore = "requires retained fixture, release build and coordinated quiet timing slot"]
fn production_retained_query_latency() {
    use kelvo_schema::durable_usage::*;
    let path = std::env::var_os("KELVO_USAGE_FILL_PATH").expect("retained fixture path");
    let store = kelvo_store::Store::open(kelvo_store::StoreConfig::new(std::path::PathBuf::from(
        path,
    )))
    .unwrap();
    for by in [
        HistoricalUsageMetric::Cpu,
        HistoricalUsageMetric::Gpu,
        HistoricalUsageMetric::Memory,
        HistoricalUsageMetric::Energy,
        HistoricalUsageMetric::Network,
    ] {
        let q = HistoricalUsageRequest {
            host: host(1).id,
            from_ms: T0,
            to_ms: T0 + 30 * DAY,
            by,
            app_names: vec![],
        };
        let mut reader = store.reader().unwrap();
        for phase in ["connection-cold", "warm"] {
            let start = std::time::Instant::now();
            let reply = reader.historical_usage_after(&q, Vec::new).unwrap();
            let elapsed = start.elapsed();
            println!(
                "[production query] app {by:?} {phase} {}ms {} rows",
                elapsed.as_millis(),
                reply.apps.len()
            );
            assert_eq!(reply.apps.len(), 10);
            assert!(
                elapsed.as_secs_f64() < 15.0,
                "remote query timeout exceeded"
            );
        }
    }
    let mut q = UsageProcessRequest {
        host: host(1).id,
        from_ms: T0,
        to_ms: T0 + 30 * DAY,
        app_name: "Application 000".into(),
        by: HistoricalUsageMetric::Cpu,
        page_size: 100,
        cursor: None,
    };
    let mut reader = store.reader().unwrap();
    for phase in ["connection-cold-first-page", "warm-next-page"] {
        let start = std::time::Instant::now();
        let reply = reader.usage_processes_after(&q, Vec::new).unwrap();
        let elapsed = start.elapsed();
        println!(
            "[production query] process {phase} {}ms {} rows continued={}",
            elapsed.as_millis(),
            reply.processes.len(),
            reply.next_cursor.is_some()
        );
        assert_eq!(reply.processes.len(), 100);
        assert!(reply.next_cursor.is_some());
        q.cursor = reply.next_cursor;
        assert!(
            elapsed.as_secs_f64() < 15.0,
            "remote query timeout exceeded"
        );
    }
}

#[test]
#[ignore = "replay captured real collector facts through production Writer"]
fn production_real_host_replay() {
    use rusqlite::params;
    let source = std::env::var_os("KELVO_USAGE_REAL_SOURCE").expect("captured fixture path");
    let db = rusqlite::Connection::open_with_flags(
        std::path::PathBuf::from(source),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let apps: std::collections::HashMap<u64, Arc<str>> = db
        .prepare("SELECT id,name FROM usage_apps")
        .unwrap()
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)? as u64,
                Arc::from(r.get::<_, String>(1)?),
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let procs:std::collections::HashMap<u64,(Arc<str>,i32,i64,Arc<str>)>=db.prepare("SELECT p.id,a.name,p.pid,p.start_time_us,p.name FROM usage_processes p JOIN usage_apps a ON a.id=p.app_id").unwrap().query_map([],|r|Ok((r.get::<_,i64>(0)? as u64,(Arc::from(r.get::<_,String>(1)?),r.get(2)?,r.get(3)?,Arc::from(r.get::<_,String>(4)?))))).unwrap().collect::<Result<_,_>>().unwrap();
    let dir = temp_dir("usage-real-replay");
    let store = open(&dir, "replay.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    w.flush().unwrap();
    let first = w.write_stats().unwrap();
    let mut stmt=db.prepare("SELECT a.bucket_ts,a.blob,p.blob,c.blob FROM usage_app_buckets a JOIN usage_process_buckets p USING(host_id,width_ms,bucket_ts) JOIN usage_coverage c USING(host_id,width_ms,bucket_ts) WHERE a.width_ms=10000 ORDER BY a.bucket_ts").unwrap();
    let mut rows = stmt.query([]).unwrap();
    let mut count = 0;
    while let Some(row) = rows.next().unwrap() {
        let at: i64 = row.get(0).unwrap();
        let a: Vec<u8> = row.get(1).unwrap();
        let p: Vec<u8> = row.get(2).unwrap();
        let c: Vec<u8> = row.get(3).unwrap();
        let source = codec::decode_selected(&c, None).unwrap()[0].1.coverage;
        let app_facts = codec::decode_selected(&a, None)
            .unwrap()
            .into_iter()
            .map(|(id, facts)| UsageAppFacts {
                name: apps[&id].clone(),
                facts,
            })
            .collect();
        let process_facts = codec::decode_selected(&p, None)
            .unwrap()
            .into_iter()
            .map(|(id, facts)| {
                let (app, pid, start, name) = &procs[&id];
                UsageProcessFacts {
                    app_name: app.clone(),
                    pid: *pid,
                    start_time_us: *start,
                    name: name.clone(),
                    facts,
                }
            })
            .collect();
        let uncertain=db.prepare("SELECT a.name,u.possible_from_ms,u.possible_to_ms,u.rx_bytes,u.tx_bytes,u.records FROM usage_uncertain u LEFT JOIN usage_apps a ON a.id=u.app_id WHERE discovery_bucket_ts=?1").unwrap().query_map([at],|r|Ok(kelvo_schema::durable_usage::UsageNetworkUncertainty{app_name:r.get(0)?,possible_from_ms:r.get(1)?,possible_to_ms:r.get(2)?,rx_bytes:r.get::<_,i64>(3)? as u64,tx_bytes:r.get::<_,i64>(4)? as u64,records:r.get::<_,i64>(5)? as u64})).unwrap().collect::<Result<_,_>>().unwrap();
        w.write_usage_bucket(
            host(1).id,
            UsageBucket {
                from_ms: at,
                to_ms: at + 10_000,
                source,
                apps: app_facts,
                processes: process_facts,
                uncertain_network: uncertain,
            },
        )
        .unwrap();
        count += 1;
        if count % 30 == 0 {
            w.flush().unwrap();
        }
    }
    w.flush().unwrap();
    let end = w.write_stats().unwrap();
    let mut payload = 0_i64;
    let mut population = 0_i64;
    let target = raw(&store);
    for table in [
        "usage_apps",
        "usage_processes",
        "usage_app_buckets",
        "usage_process_buckets",
        "usage_coverage",
        "usage_uncertain",
        "usage_recording_start",
    ] {
        let columns: Vec<(String, String)> = target
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| Ok((r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let expression = columns
            .iter()
            .map(|(name, ty)| {
                if ty == "INTEGER" {
                    format!("CASE WHEN {name} IS NULL THEN 0 ELSE 8 END")
                } else {
                    format!("coalesce(length({name}),0)")
                }
            })
            .collect::<Vec<_>>()
            .join("+");
        let (n, bytes): (i64, i64) = target
            .query_row(
                &format!("SELECT count(*),coalesce(sum({expression}),0) FROM {table}"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        population += n;
        payload += bytes;
        println!("[real replay] {table} rows={n} payload={bytes}");
    }
    let wal = end.wal_bytes() - first.wal_bytes();
    println!(
        "[real replay] {count} finest frames total usage rows={population} payload={payload} WAL={wal}; baseline-added payload={} WAL={}",
        payload + 769392,
        wal + 4331712
    );
    assert!(payload + 769392 <= 4_000_000);
    assert!(wal + 4331712 <= 12_000_000);
    let (from, to): (i64, i64) = db
        .query_row(
            "SELECT min(bucket_ts),max(bucket_ts)+10000 FROM usage_coverage",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let q = kelvo_schema::durable_usage::HistoricalUsageRequest {
        host: host(1).id,
        from_ms: from,
        to_ms: to,
        by: kelvo_schema::durable_usage::HistoricalUsageMetric::Cpu,
        app_names: vec![],
    };
    assert_eq!(
        store
            .reader()
            .unwrap()
            .historical_usage_after(&q, Vec::new)
            .unwrap()
            .apps
            .len(),
        10
    );
    let (n,): (i64,) = target
        .query_row(
            "SELECT count(*) FROM usage_app_buckets WHERE width_ms=?1",
            params![10000],
            |r| Ok((r.get(0)?,)),
        )
        .unwrap();
    assert!(n < count);
}
