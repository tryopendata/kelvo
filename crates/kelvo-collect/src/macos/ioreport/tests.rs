use kelvo_schema::{Labels, MetricId, SeriesKey};

use super::channels::*;
use super::plan::*;
use super::*;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

fn meta(group: &str, name: &str, unit: &str, states: Vec<String>) -> ChannelMeta {
    ChannelMeta {
        group: group.into(),
        name: name.into(),
        unit: unit.into(),
        states,
    }
}

fn cpu_states(n: usize) -> Vec<String> {
    let mut v = s(&["DOWN", "IDLE"]);
    v.extend((0..n).map(|i| format!("V{i}P{}", n - 1 - i)));
    v
}

/// Channel layout as subscribed on the development M3 Max (macOS 27.0.1).
fn m3_max() -> (Vec<ChannelMeta>, soc::DvfsTables) {
    let mut gpu = s(&["OFF"]);
    gpu.extend((1..=15).map(|i| format!("P{i}")));
    let metas = vec![
        meta("CPU Stats", "ECPU", "24Mticks", cpu_states(6)),
        meta("CPU Stats", "PCPU", "24Mticks", cpu_states(20)),
        meta("CPU Stats", "PCPU1", "24Mticks", cpu_states(20)),
        meta("GPU Stats", "GPUPH", "24Mticks", gpu),
        meta("Energy Model", "EACC_CPU", "mJ", vec![]),
        meta("Energy Model", "PACC0_CPU", "mJ", vec![]),
        meta("Energy Model", "PACC1_CPU", "mJ", vec![]),
        meta("Energy Model", "CPU Energy", "mJ", vec![]),
        meta("Energy Model", "ANE0", "mJ", vec![]),
        meta("Energy Model", "DRAM0", "mJ", vec![]),
        meta("Energy Model", "GPU Energy", "nJ", vec![]),
    ];
    let tables = soc::DvfsTables {
        ecpu_mhz: vec![1020, 1320, 1704, 2088, 2484, 2568],
        pcpu_mhz: vec![
            1092, 1356, 1596, 1884, 2172, 2424, 2616, 2808, 2988, 3144, 3288, 3420, 3516, 3576,
            3624, 3708, 3780, 3864, 3960, 4056,
        ],
        gpu_mhz: vec![
            0, 338, 618, 796, 832, 924, 952, 1056, 1064, 1182, 1182, 1312, 1242, 1380,
        ],
    };
    (metas, tables)
}

struct Fake {
    ints: Vec<i64>,
    res: Vec<Vec<i64>>,
}

impl Readings for Fake {
    fn len(&self) -> usize {
        self.ints.len()
    }
    fn integer(&self, i: usize) -> i64 {
        self.ints[i]
    }
    fn residency(&self, i: usize, state: usize) -> i64 {
        self.res[i].get(state).copied().unwrap_or(0)
    }
}

/// One second on the M3 Max layout. `cpu_mj` is the "CPU Energy" delta; the cluster
/// channels split it 1:4:5.
fn tick(cpu_mj: i64) -> Fake {
    let mut res = vec![Vec::new(); 11];
    // ECPU: 50% idle, 25% at 1020, 25% at 2568.
    res[0] = vec![100, 400, 250, 0, 0, 0, 0, 250];
    // PCPU: all DOWN/IDLE.
    res[1] = vec![600, 400];
    // PCPU1: 90% idle, 10% at 4056 (the last active state).
    let mut p1 = vec![0, 900];
    p1.extend(std::iter::repeat_n(0, 19));
    p1.push(100);
    res[2] = p1;
    // GPU: 80% off, 10% at P1 (338), 5% at P9 and P10 (both 1182), 5% at P14 (beyond
    // the table).
    let mut g = vec![800, 100];
    g.extend(std::iter::repeat_n(0, 7));
    g.extend([25, 25]);
    g.extend([0, 0, 0]);
    g.push(50);
    res[3] = g;
    let pmp = |v: i64| if cpu_mj > 0 { v } else { 0 };
    // Indexes 0-3 are the state channels (no integer value), 4-10 the energy channels
    // in `m3_max` order.
    let ints = vec![
        0,
        0,
        0,
        0,
        cpu_mj / 10,
        cpu_mj * 4 / 10,
        cpu_mj * 5 / 10,
        cpu_mj,
        pmp(100),
        pmp(400),
        2_500_000_000, // GPU Energy, nJ: 2.5 W over 1 s
    ];
    Fake { ints, res }
}

fn key(id: &'static str, labels: &[(&str, &str)]) -> SeriesKey {
    let labels = Labels::from_pairs(labels.iter().copied()).unwrap();
    SeriesKey::new(MetricId::from_static(id), labels)
}

fn run(plan: &Plan, acc: &mut Accum, r: &Fake) -> SampleBuf {
    let mut out = SampleBuf::new();
    reduce(plan, acc, r, Duration::from_secs(1), &mut out).unwrap();
    out
}

#[test]
fn parses_cluster_channel_names() {
    let id = |kind, die, n| Some(ClusterId { kind, die, n });
    assert_eq!(parse_cluster_channel("ECPU"), id(ClusterKind::E, 0, 0));
    assert_eq!(parse_cluster_channel("PCPU1"), id(ClusterKind::P, 0, 1));
    assert_eq!(parse_cluster_channel("MCPU"), id(ClusterKind::M, 0, 0));
    // Ultra names, unverified on hardware.
    assert_eq!(
        parse_cluster_channel("DIE_1_PCPU1"),
        id(ClusterKind::P, 1, 1)
    );
    for not_a_cluster in [
        "ECPM",
        "PCPM_IDLE",
        "PCPU010",
        "ECPM_IDLE",
        "GPUPH",
        "DIE_x_PCPU",
    ] {
        assert_eq!(
            parse_cluster_channel(not_a_cluster),
            None,
            "{not_a_cluster}"
        );
    }
    assert_eq!(parse_cluster_energy("EACC_CPU"), id(ClusterKind::E, 0, 0));
    assert_eq!(parse_cluster_energy("PACC1_CPU"), id(ClusterKind::P, 0, 1));
    assert_eq!(
        parse_cluster_energy("DIE_1_PACC0_CPU"),
        id(ClusterKind::P, 1, 0)
    );
    for no in ["EACC_CPU0", "PACC0_CPM", "EACC_CPU0_SRAM", "CPU Energy"] {
        assert_eq!(parse_cluster_energy(no), None, "{no}");
    }
    assert_eq!(energy_role("ANE0"), Some(EnergyRole::Ane));
    assert_eq!(energy_role("ANE0_SRAM"), None);
    assert_eq!(energy_role("DRAM0"), Some(EnergyRole::Dram));
    assert_eq!(
        energy_role("GPU0"),
        None,
        "PMP GPU channel; GPU Energy is used"
    );
    assert!(keep_channel("GPU Stats", "GPUPH"));
    assert!(!keep_channel("GPU Stats", "GPU_SW"));
}

#[test]
fn plan_labels_clusters_and_lists_series() {
    let (metas, tables) = m3_max();
    let plan = Plan::new(&metas, &tables, Groups::ALL);
    let series = plan.series();
    for k in [
        key("cpu.cluster.freq", &[("cluster", "E0")]),
        key("cpu.cluster.active", &[("cluster", "P1")]),
        key(
            "cpu.cluster.residency",
            &[("cluster", "P0"), ("state", "idle")],
        ),
        key(
            "cpu.cluster.residency",
            &[("cluster", "P0"), ("state", "4056")],
        ),
        key("cpu.cluster.power", &[("cluster", "P1")]),
        key("gpu.residency", &[("state", "1182")]),
        key("power.package", &[]),
        key("power.ane", &[]),
    ] {
        assert!(series.contains(&k), "missing {k}");
    }
    // E0: idle + 6 MHz states; GPU: idle + 12 unique MHz (1182 appears twice).
    let count = |id: &str| series.iter().filter(|k| k.metric.as_str() == id).count();
    assert_eq!(count("cpu.cluster.residency"), 7 + 21 + 21);
    assert_eq!(count("gpu.residency"), 13);
    assert_eq!(
        series.len(),
        series
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
}

#[test]
fn disabled_groups_emit_nothing_from_them() {
    let (metas, tables) = m3_max();
    let plan = Plan::new(
        &metas,
        &tables,
        Groups {
            cpu: false,
            gpu: true,
            energy: false,
            cpu_power: true,
        },
    );
    let series = plan.series();
    assert!(series.iter().all(|k| k.metric.as_str().starts_with("gpu.")));
    assert!(!series.is_empty());
}

#[test]
fn reduces_residency_frequency_and_power() {
    let (metas, tables) = m3_max();
    let plan = Plan::new(&metas, &tables, Groups::ALL);
    let mut acc = Accum::default();
    let out = run(&plan, &mut acc, &tick(10_000));

    let get = |k: SeriesKey| out.get(&k).unwrap();
    // ECPU: half the time at 1020 and 2568 -> 1794 MHz, 50% active.
    assert_eq!(get(key("cpu.cluster.freq", &[("cluster", "E0")])), 1_794e6);
    assert_eq!(get(key("cpu.cluster.active", &[("cluster", "E0")])), 50.0);
    let res = |c: &str| -> f32 {
        out.values()
            .iter()
            .filter(|s| {
                s.key.metric.as_str() == "cpu.cluster.residency"
                    && s.key.labels.get("cluster") == Some(c)
            })
            .map(|s| s.value)
            .sum()
    };
    for c in ["E0", "P0", "P1"] {
        assert!(
            (res(c) - 100.0).abs() < 1e-3,
            "{c} residency sums to {}",
            res(c)
        );
    }
    // Fully idle cluster reports its minimum frequency, 0% active.
    assert_eq!(get(key("cpu.cluster.freq", &[("cluster", "P0")])), 1_092e6);
    assert_eq!(get(key("cpu.cluster.active", &[("cluster", "P0")])), 0.0);
    assert_eq!(get(key("cpu.cluster.freq", &[("cluster", "P1")])), 4_056e6);
    // GPU: 20% active; P14 has no table entry, so the frequency averages the mapped
    // 15%: (10*338 + 5*1182) / 15 = 619.3 MHz. Residency of mapped states sums to 95.
    let gpu_f = get(key("gpu.freq", &[]));
    assert!((gpu_f - 619.333e6).abs() < 1e3, "{gpu_f}");
    assert_eq!(get(key("gpu.residency", &[("state", "1182")])), 5.0);
    assert_eq!(get(key("gpu.residency", &[("state", "idle")])), 80.0);
    // Power over one second.
    assert_eq!(get(key("power.cpu", &[])), 10.0);
    assert_eq!(get(key("power.gpu", &[])), 2.5);
    assert_eq!(get(key("power.ane", &[])), 0.1);
    assert_eq!(get(key("power.dram", &[])), 0.4);
    assert_eq!(get(key("cpu.cluster.power", &[("cluster", "P1")])), 5.0);
    assert_eq!(get(key("power.package", &[])), 13.0);
    assert!(!acc.pmp_stale);
}

#[test]
fn stale_pmp_counters_emit_no_cpu_power() {
    let (metas, tables) = m3_max();
    let plan = Plan::new(&metas, &tables, Groups::ALL);
    let mut acc = Accum::default();
    let cpu = key("power.cpu", &[]);
    let gpu = key("power.gpu", &[]);
    let pkg = key("power.package", &[]);

    // Counters frozen for four ticks: GPU power still flows, CPU-side power does not.
    for _ in 0..4 {
        let out = run(&plan, &mut acc, &tick(0));
        assert_eq!(
            out.get(&cpu),
            None,
            "0 W from a frozen counter is not a reading"
        );
        assert_eq!(out.get(&pkg), None);
        assert_eq!(out.get(&gpu), Some(2.5));
        assert!(acc.pmp_stale);
    }
    // The refresh: five minutes' worth of energy lands in one tick. Not this tick's
    // power, so skipped.
    let out = run(&plan, &mut acc, &tick(5_481_355));
    assert_eq!(out.get(&cpu), None, "a multi-tick jump must not be emitted");
    assert!(acc.pmp_stale);
    // Counters moving every tick again: values resume.
    let out = run(&plan, &mut acc, &tick(10_000));
    assert_eq!(out.get(&cpu), Some(10.0));
    assert!(!acc.pmp_stale);
}

#[test]
fn smc_cpu_power_leaves_only_cpu_series_to_the_smc() {
    let (metas, tables) = m3_max();
    let groups = Groups {
        cpu_power: false,
        ..Groups::ALL
    };
    let plan = Plan::new(&metas, &tables, groups);
    let series = plan.series();
    let cpu = key("power.cpu", &[]);
    let p1 = key("cpu.cluster.power", &[("cluster", "P1")]);
    assert!(!series.contains(&cpu));
    assert!(
        !series
            .iter()
            .any(|k| k.metric.as_str() == "cpu.cluster.power")
    );
    for k in ["power.gpu", "power.ane", "power.dram", "power.package"] {
        assert!(series.contains(&key(k, &[])), "{k} stays on IOReport");
    }

    // The PMP gating still runs on the CPU energy channels: frozen counters mean no
    // ANE, DRAM or package value; moving ones bring them back.
    let mut acc = Accum::default();
    let out = run(&plan, &mut acc, &tick(0));
    assert_eq!(out.get(&key("power.dram", &[])), None);
    assert_eq!(out.get(&key("power.gpu", &[])), Some(2.5));
    assert!(acc.pmp_stale);
    assert!(!acc.pmp_moved);
    let out = run(&plan, &mut acc, &tick(10_000));
    // The calibration gets the P clusters' energy only (4 J + 5 J), not EACC's 1 J.
    assert!(acc.pmp_moved);
    assert!((acc.p_cluster_j - 9.0).abs() < 1e-9, "{}", acc.p_cluster_j);
    assert_eq!(out.get(&key("power.dram", &[])), Some(0.4));
    assert_eq!(out.get(&key("power.package", &[])), Some(13.0));
    assert_eq!(out.get(&cpu), None);
    assert_eq!(out.get(&p1), None);
}

#[test]
fn changed_channel_count_is_an_error() {
    let (metas, tables) = m3_max();
    let plan = Plan::new(&metas, &tables, Groups::ALL);
    let mut r = tick(10_000);
    r.ints.pop();
    let mut out = SampleBuf::new();
    let err = reduce(
        &plan,
        &mut Accum::default(),
        &r,
        Duration::from_secs(1),
        &mut out,
    );
    assert!(matches!(err, Err(CollectError::UnexpectedShape { .. })));
    assert!(out.values().is_empty());
}

/// Samples the real hardware twice and checks plausibility.
#[test]
#[ignore = "needs Apple Silicon hardware; run by hand with --ignored --nocapture"]
fn live_ioreport() {
    let chip = soc::chip_name().unwrap_or_default();
    let tables = soc::dvfs_tables(&chip).expect("DVFS tables");
    println!("chip: {chip}\nDVFS: {tables:?}");
    let mut c = IoReport::new(Groups::ALL);
    let Probe::Supported(series) = c.probe() else {
        panic!("IOReport unsupported");
    };
    println!("{} series", series.len());
    let all_cpu: Vec<u32> = tables
        .ecpu_mhz
        .iter()
        .chain(&tables.pcpu_mhz)
        .copied()
        .collect();
    let (lo, hi) = (
        f64::from(*all_cpu.iter().min().unwrap()),
        f64::from(*all_cpu.iter().max().unwrap()),
    );
    for n in 0..2 {
        std::thread::sleep(Duration::from_secs(1));
        let mut out = SampleBuf::new();
        let tick = Tick {
            n,
            wall_ms: 0,
            continuous_ns: 0,
            interval_ms: 1_000,
        };
        c.sample(&tick, &mut out).unwrap();
        println!("--- sample {n} (PMP stale: {})", c.pmp_stale());
        let mut residency: std::collections::BTreeMap<String, f32> = Default::default();
        for s in out.values() {
            let id = s.key.metric.as_str();
            if id == "cpu.cluster.residency" || id == "gpu.residency" {
                let who = s.key.labels.get("cluster").unwrap_or("gpu").to_owned();
                *residency.entry(who).or_default() += s.value;
                if s.value > 0.5 {
                    println!("  {} = {:.1}", s.key, s.value);
                }
                continue;
            }
            println!("  {} = {:.3}", s.key, s.value);
            match id {
                "cpu.cluster.freq" => {
                    let f = f64::from(s.value);
                    assert!(
                        (lo * 1e6 - 1.0..=hi * 1e6 + 1.0).contains(&f),
                        "{} {f}",
                        s.key
                    );
                }
                "gpu.freq" => {
                    let max = f64::from(*tables.gpu_mhz.iter().max().unwrap()) * 1e6;
                    assert!((0.0..=max + 1.0).contains(&f64::from(s.value)));
                }
                "cpu.cluster.active" => assert!((0.0..=100.0).contains(&s.value)),
                _ if id.starts_with("power.") || id == "cpu.cluster.power" => {
                    assert!(s.value >= 0.0 && s.value < 500.0, "{} {}", s.key, s.value);
                }
                _ => {}
            }
        }
        for (who, sum) in &residency {
            println!("  residency sum {who} = {sum:.2}");
            // GPU states beyond the DVFS table are not labelled, so its sum may be lower.
            if who == "gpu" {
                assert!(*sum <= 100.01 && *sum > 90.0, "{who} {sum}");
            } else {
                assert!((sum - 100.0).abs() < 0.01, "{who} {sum}");
            }
        }
    }
}

/// Runs the SMC power collector and IOReport together at 1 s, as the engine does, and
/// checks the calibrated `power.cpu` against PMP over whole refresh windows that
/// started after the first calibration. That is an out-of-sample check: each window's
/// scale comes from earlier windows. Takes three or more PMP refreshes (15 to 45 min);
/// `KELVO_CALIB_MINUTES` caps it (default 60).
#[test]
#[ignore = "needs an M3 Max; runs up to an hour; run by hand with --ignored --nocapture"]
fn live_cpu_power_calibration_matches_pmp() {
    use super::super::smc;
    use super::super::sysctl::continuous_ns;
    let minutes: u64 = std::env::var("KELVO_CALIB_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let source = CpuPowerSource::default();
    let mut power = smc::Power::new(source.clone());
    let mut ior = IoReport::new(Groups::ALL).with_cpu_power(source.clone());
    assert!(matches!(power.probe(), Probe::Supported(_)));
    assert!(source.smc(), "this chip has no SMC CPU power map");
    assert!(matches!(ior.probe(), Probe::Supported(_)));
    let cpu = SeriesKey::bare(MetricId::from_static("power.cpu"));
    let status = SeriesKey::bare(MetricId::from_static("power.cpu_source"));

    let start = std::time::Instant::now();
    let mut last_ns = continuous_ns();
    // Calibrated SMC energy since the last PMP move, if the whole span was calibrated.
    let mut smc_j: Option<f64> = None;
    let mut errors = Vec::new();
    let mut n = 0;
    while start.elapsed() < Duration::from_secs(minutes * 60) && errors.len() < 3 {
        std::thread::sleep(Duration::from_secs(1));
        n += 1;
        let now = continuous_ns();
        let tick = Tick {
            n,
            wall_ms: 0,
            continuous_ns: now,
            interval_ms: 1_000,
        };
        let mut out = SampleBuf::new();
        power.sample(&tick, &mut out).unwrap();
        let dt = (now - last_ns) as f64 / 1e9;
        last_ns = now;
        let calibrated = out.get(&status)
            == Some(kelvo_schema::MetricCode::value(
                kelvo_schema::CpuPowerCalibration::Calibrated,
            ));
        match (out.get(&cpu), calibrated) {
            (Some(w), true) => smc_j = smc_j.map(|j| j + f64::from(w) * dt),
            _ => smc_j = None,
        }
        let mut out2 = SampleBuf::new();
        ior.sample(&tick, &mut out2).unwrap();
        let acc = &ior.active.as_ref().unwrap().acc;
        if acc.pmp_moved {
            let scale = source.calib().scale();
            let t = start.elapsed().as_secs();
            if let Some(j) = smc_j {
                let err = j / acc.p_cluster_j - 1.0;
                println!(
                    "{t:>5} s  PMP {:.1} J, calibrated SMC {j:.1} J, error {:+.1}%, scale now {scale:?}",
                    acc.p_cluster_j,
                    err * 100.0
                );
                errors.push(err);
            } else {
                println!("{t:>5} s  PMP moved, scale {scale:?}");
            }
            smc_j = calibrated.then_some(0.0);
        }
    }
    println!("errors: {errors:?}");
    assert!(
        !errors.is_empty(),
        "no calibrated window closed in {minutes} min"
    );
    for e in errors {
        assert!(e.abs() <= 0.05, "{:+.1}%", e * 100.0);
    }
}
