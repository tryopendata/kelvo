//! `query_energy_by_app` (D-093): the engine's per-app energy over a range, with what
//! the Quit actions need. Which process an app row's Quit acts on is decided here, so
//! every view that offers it agrees.

use kelvo_engine::{EnergyApp, EnergyProc};

use crate::ipc::{AppEnergy, EnergyByApp, ProcessEnergy};
use crate::process_signal::SignalRefusal;

/// The process an app row's Quit acts on: the running main executable (quitting it
/// quits the app and its helpers), else the only running process.
fn quit_pid(processes: &[EnergyProc]) -> Option<i32> {
    let mut running = processes.iter().filter(|p| p.running);
    if let Some(main) = processes.iter().find(|p| p.running && p.app_main) {
        return Some(main.pid);
    }
    match (running.next(), running.next()) {
        (Some(only), None) => Some(only.pid),
        _ => None,
    }
}

/// The IPC answer. `refusal` is the rule `process_signal` checks, for a running process
/// `(pid, start_time_us, name)`.
pub fn energy_by_app(
    e: kelvo_engine::EnergyByApp,
    refusal: impl Fn(i32, i64, &str) -> Option<SignalRefusal>,
) -> EnergyByApp {
    let app = |a: EnergyApp| AppEnergy {
        name: a.name.to_string(),
        energy_j: a.joules,
        avg_w: a.avg_w,
        quit_pid: quit_pid(&a.processes),
        processes: a
            .processes
            .iter()
            .map(|p| ProcessEnergy {
                pid: p.pid,
                start_time_us: p.start_time_us,
                name: p.name.to_string(),
                energy_j: p.joules,
                avg_w: p.avg_w,
                running: p.running,
                refusal: if p.running {
                    refusal(p.pid, p.start_time_us, &p.name)
                } else {
                    None
                },
            })
            .collect(),
    };
    EnergyByApp {
        from_ms: e.from_ms,
        to_ms: e.to_ms,
        since_ms: e.since_ms,
        measured_ms: e.measured_ms,
        total_j: e.total_j,
        apps: e.apps.into_iter().map(app).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: i32, running: bool, main: bool) -> EnergyProc {
        EnergyProc {
            pid,
            start_time_us: 1,
            name: format!("p{pid}").into(),
            joules: 1.0,
            avg_w: 0.1,
            running,
            app_main: main,
        }
    }

    #[test]
    fn quit_targets_the_main_executable_then_a_sole_process() {
        // Chrome: helpers and the main process; Quit goes to the main one.
        assert_eq!(quit_pid(&[p(2, true, false), p(1, true, true)]), Some(1));
        // The main process exited: no single target among the helpers left.
        assert_eq!(
            quit_pid(&[p(2, true, false), p(3, true, false), p(1, false, true)]),
            None
        );
        // A CLI with one running process.
        assert_eq!(quit_pid(&[p(5, false, false), p(6, true, false)]), Some(6));
        assert_eq!(quit_pid(&[p(5, false, false)]), None);
    }

    #[test]
    fn exited_processes_carry_no_refusal() {
        let e = kelvo_engine::EnergyByApp {
            from_ms: 0,
            to_ms: 10_000,
            since_ms: Some(0),
            measured_ms: 10_000,
            total_j: 2.0,
            apps: vec![EnergyApp {
                name: "WindowServer".into(),
                joules: 2.0,
                avg_w: 0.2,
                processes: vec![p(1, true, false), p(2, false, false)],
            }],
        };
        let out = energy_by_app(e, |_, _, _| Some(SignalRefusal::WindowServer));
        let procs = &out.apps[0].processes;
        assert_eq!(procs[0].refusal, Some(SignalRefusal::WindowServer));
        assert_eq!(procs[1].refusal, None);
        assert_eq!(out.apps[0].quit_pid, Some(1));
    }
}
