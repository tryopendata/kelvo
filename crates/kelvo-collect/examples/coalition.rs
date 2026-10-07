//! CPU and memory of a running app's coalition: the app process plus every process macOS
//! holds it responsible for (its WebKit helpers), with the same membership rule as the
//! `self.cpu` collector. Used by `scripts/bench-coalition.sh` (D-067).
//!
//! ```text
//! cargo run --release -p kelvo-collect --example coalition -- --pid 4242 --warmup 20 --seconds 120
//! ```
//!
//! Every 2 s it lists the members and adds up each one's CPU time since the previous
//! listing (a helper that exits keeps what it used until then; one that starts counts
//! from its start). After the run it prints one JSON line: `cpu_pct` (percent of one core
//! over the measured span), `footprint_mb` (sum of `phys_footprint` at the end), the
//! same per process name, and `app_threads`: the app process's CPU by thread name
//! (`main` is AppKit and the tray image, `kelvo-engine` the sampler, `tokio-rt-worker`
//! the live channels and commands).

#[cfg(target_os = "macos")]
fn main() {
    if let Err(e) = mac::run() {
        eprintln!("coalition: {e}");
        std::process::exit(2);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("coalition: macOS only");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
mod mac {
    use std::collections::{BTreeMap, HashMap};
    use std::time::{Duration, Instant};

    use kelvo_collect::macos::self_cpu::{Coalition, Member, process_name};

    const EVERY: Duration = Duration::from_secs(2);

    struct Args {
        pid: i32,
        warmup: u64,
        seconds: u64,
    }

    fn args() -> Result<Args, String> {
        let mut a = Args {
            pid: 0,
            warmup: 20,
            seconds: 120,
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut num = |name: &str| -> Result<u64, String> {
                it.next()
                    .ok_or(format!("{name} N"))?
                    .parse()
                    .map_err(|e| format!("{name}: {e}"))
            };
            match flag.as_str() {
                "--pid" => a.pid = i32::try_from(num("--pid")?).map_err(|e| e.to_string())?,
                "--warmup" => a.warmup = num("--warmup")?,
                "--seconds" => a.seconds = num("--seconds")?,
                other => return Err(format!("unknown flag {other}")),
            }
        }
        if a.pid <= 0 {
            return Err("--pid is required".into());
        }
        Ok(a)
    }

    /// User plus system CPU ns of each thread of `pid`, summed by thread name (GCD
    /// workers and other unnamed threads as `unnamed`). Threads that exit during the run
    /// take their time with them, so this attributes, it does not total.
    fn thread_times(pid: i32) -> BTreeMap<String, u64> {
        // sys/proc_info.h: lists the thread handles (u64 each) for PROC_PIDTHREADINFO.
        const PROC_PIDLISTTHREADS: libc::c_int = 6;
        let mut ids = vec![0u64; 512];
        let bytes = (ids.len() * std::mem::size_of::<u64>()) as libc::c_int;
        // SAFETY: the buffer holds `bytes` bytes and outlives the call.
        let n = unsafe {
            libc::proc_pidinfo(pid, PROC_PIDLISTTHREADS, 0, ids.as_mut_ptr().cast(), bytes)
        };
        let count = usize::try_from(n).unwrap_or(0) / std::mem::size_of::<u64>();
        let mut out = BTreeMap::new();
        for &id in ids.iter().take(count) {
            // SAFETY: a plain C struct; all zeroes is a valid value.
            let mut ti: libc::proc_threadinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_threadinfo>() as libc::c_int;
            // SAFETY: `ti` is `size` bytes and outlives the call.
            let got = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTHREADINFO,
                    id,
                    (&raw mut ti).cast(),
                    size,
                )
            };
            if got != size {
                continue;
            }
            let name: Vec<u8> = ti
                .pth_name
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            let name = if name.is_empty() {
                "unnamed".to_string()
            } else {
                String::from_utf8_lossy(&name).into_owned()
            };
            *out.entry(name).or_insert(0) += ti.pth_user_time + ti.pth_system_time;
        }
        out
    }

    /// A run whose wall clock ran more than [`SLEPT_S`] ahead of its monotonic clock
    /// spans a sleep: what it measured is not the scenario, so it is not a result.
    fn slept_check(start_wall: std::time::SystemTime, awake_s: f64) -> Result<(), String> {
        let wall_s = start_wall.elapsed().map_or(awake_s, |d| d.as_secs_f64());
        if wall_s - awake_s > SLEPT_S {
            return Err(format!(
                "the Mac slept during the run ({wall_s:.0} s of wall time, {awake_s:.0} s awake)"
            ));
        }
        Ok(())
    }

    const SLEPT_S: f64 = 5.0;

    fn now_ns(t0: Instant) -> u64 {
        u64::try_from(t0.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    pub fn run() -> Result<(), String> {
        let a = args()?;
        let t0 = Instant::now();
        let mut c = Coalition::new(a.pid);
        let mut members: Vec<Member> = Vec::new();
        let alive = |c: &mut Coalition, m: &mut Vec<Member>| -> Result<(), String> {
            if !c.members(now_ns(t0), m) {
                return Err("listing pids failed".into());
            }
            if !m.iter().any(|x| x.pid == a.pid) {
                return Err(format!("pid {} is gone", a.pid));
            }
            Ok(())
        };
        alive(&mut c, &mut members)?;
        std::thread::sleep(Duration::from_secs(a.warmup));

        // (pid, start) -> CPU ns at the last listing, and CPU ns used per name since.
        let mut last: HashMap<(i32, u64), u64> = HashMap::new();
        let mut names: HashMap<(i32, u64), String> = HashMap::new();
        let mut used: BTreeMap<String, u64> = BTreeMap::new();
        alive(&mut c, &mut members)?;
        for m in &members {
            last.insert((m.pid, m.start_abstime), m.cpu_ns);
        }
        let threads0 = thread_times(a.pid);
        let start = Instant::now();
        // `Instant` stops while the Mac sleeps; the wall clock does not.
        let start_wall = std::time::SystemTime::now();
        let end = start + Duration::from_secs(a.seconds);
        let mut footprint: BTreeMap<String, u64> = BTreeMap::new();
        loop {
            let left = end.saturating_duration_since(Instant::now());
            std::thread::sleep(left.min(EVERY));
            alive(&mut c, &mut members)?;
            footprint.clear();
            for m in &members {
                let key = (m.pid, m.start_abstime);
                let name = names
                    .entry(key)
                    .or_insert_with(|| process_name(m.pid).unwrap_or_else(|| m.pid.to_string()))
                    .clone();
                // A member first seen now started during the run (or was not a member
                // at the last listing): everything since its start counts.
                let before = last.get(&key).copied().unwrap_or(0);
                *used.entry(name.clone()).or_insert(0) += m.cpu_ns.saturating_sub(before);
                last.insert(key, m.cpu_ns);
                *footprint.entry(name).or_insert(0) += m.footprint_bytes;
            }
            if Instant::now() >= end {
                break;
            }
        }
        let threads1 = thread_times(a.pid);
        let wall = start.elapsed().as_secs_f64();
        slept_check(start_wall, wall)?;
        let pct = |ns: u64| ns as f64 / 1e9 / wall * 100.0;
        let threads: Vec<String> = threads1
            .iter()
            .map(|(n, &ns)| (n, ns.saturating_sub(threads0.get(n).copied().unwrap_or(0))))
            .filter(|(_, ns)| *ns > 0)
            .map(|(n, ns)| format!("{n:?}: {:.4}", pct(ns)))
            .collect();
        let total: u64 = used.values().sum();
        let fp_total: u64 = footprint.values().sum();
        let mb = |b: u64| b as f64 / 1e6;
        let per: Vec<String> = used
            .iter()
            .map(|(n, &ns)| {
                format!(
                    "{:?}: {{\"cpu_pct\": {:.4}, \"footprint_mb\": {:.1}}}",
                    n,
                    pct(ns),
                    mb(footprint.get(n).copied().unwrap_or(0))
                )
            })
            .collect();
        println!(
            "{{\"cpu_pct\": {:.4}, \"footprint_mb\": {:.1}, \"wall_s\": {:.1}, \"members\": {}, \"lookups\": {}, \"processes\": {{{}}}, \"app_threads\": {{{}}}}}",
            pct(total),
            mb(fp_total),
            wall,
            members.len(),
            c.lookups(),
            per.join(", "),
            threads.join(", ")
        );
        Ok(())
    }
}
