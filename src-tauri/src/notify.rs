//! Alert notifications (v1.2) through `tauri-plugin-notification`. The engine records
//! a fired alert as an `alert` event; the host watcher (state.rs) passes it here.
//!
//! What the plugin does on macOS, read from its source (2.5.1) and observed (D-084):
//! it posts through `notify-rust` (`NSUserNotificationCenter`), under Terminal's bundle id
//! in `tauri dev` and under the app's identifier in a bundled build. It has no
//! permission step on desktop (`request_permission` resolves Granted without asking;
//! macOS asks on the first delivery) and drops click actions on desktop, so a click
//! cannot be routed to the Timeline through it.
//!
//! Because macOS asks on the first delivery, switching a rule on posts a confirmation
//! ([`alerts_on`]) right away: the permission prompt then appears while the user is in
//! Settings, not with the first real alert, which may be hours later.

use kelvo_schema::{
    AlertCause, AlertRule, AlertSettings, Condition, Event, EventDetail, ThermalState,
};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Called when an alert rule is switched on. The plugin resolves at once on desktop; the
/// call stays so a plugin that gains a desktop permission step asks at the right moment.
pub fn request_permission(app: &AppHandle) {
    match app.notification().request_permission() {
        Ok(state) => tracing::info!(?state, "notification permission"),
        Err(e) => tracing::warn!("requesting notification permission: {e}"),
    }
}

/// Posts a one-off confirmation that `rules` were switched on. On macOS the first
/// delivery is what shows the permission prompt, so this makes it appear now.
pub fn alerts_on(app: &AppHandle, rules: &[AlertRule]) {
    let Some((title, body)) = alerts_on_text(rules) else {
        return;
    };
    post(app, title, body, "posting the alerts-on confirmation");
}

/// Posts the notification for an `alert` event. Other events post nothing.
pub fn alert(app: &AppHandle, event: &Event) {
    let Some((title, body)) = text(event) else {
        return;
    };
    post(app, title, body, "posting an alert notification");
}

fn post(app: &AppHandle, title: String, body: String, what: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("{what}: {e}");
    }
}

/// Title and body of the confirmation for rules just switched on, `None` for none.
pub fn alerts_on_text(rules: &[AlertRule]) -> Option<(String, String)> {
    let when: Vec<String> = rules.iter().map(describe).collect();
    let list = match when.as_slice() {
        [] => return None,
        [one] => one.clone(),
        [init @ .., last] => format!("{} or when {last}", init.join(", when ")),
    };
    Some((
        "Alerts are on".to_owned(),
        format!("You'll get a notification when {list}."),
    ))
}

/// What a rule waits for, as the end of "You'll get a notification when ...".
fn describe(rule: &AlertRule) -> String {
    match &rule.when {
        Condition::ProcessCpuAbove { percent_of_core } => {
            let minutes = rule.for_secs / 60;
            format!(
                "a process stays above {percent_of_core:.0}% CPU for {minutes} {}",
                if minutes == 1 { "minute" } else { "minutes" }
            )
        }
        Condition::ThermalStateAtLeast(level) if *level == ThermalState::Critical => {
            "the thermal state reaches Critical".to_owned()
        }
        Condition::ThermalStateAtLeast(level) => {
            format!(
                "the thermal state reaches {} or worse",
                thermal_label(*level)
            )
        }
        Condition::Threshold { .. } => rule.name.to_lowercase(),
    }
}

/// The CPU threshold of the built-in rule `rule_id`, when it is a process CPU rule.
fn cpu_threshold(rule_id: uuid::Uuid) -> Option<f32> {
    AlertSettings::default()
        .rules()
        .into_iter()
        .find(|r| r.id == rule_id)
        .and_then(|r| match r.when {
            Condition::ProcessCpuAbove { percent_of_core } => Some(percent_of_core),
            _ => None,
        })
}

/// Title and body for an alert, `None` for any other event.
pub fn text(event: &Event) -> Option<(String, String)> {
    let EventDetail::Alert { cause, rule_id, .. } = &event.detail else {
        return None;
    };
    let minutes = (event.ts_ms - event.start_ms) / 60_000;
    Some(match cause {
        AlertCause::ProcessCpu { process, cpu_pct } => {
            // One alert names every process whose run completed on that tick, hottest
            // first (`cause` is the hottest).
            let others: Vec<&str> = event
                .processes
                .iter()
                .map(String::as_str)
                .filter(|p| p != process)
                .collect();
            // A rule id this build does not know (a newer build's rule) has no threshold
            // to name.
            let (held, also) = match cpu_threshold(*rule_id) {
                Some(t) => (
                    format!("Above {t:.0}% for {minutes} minutes."),
                    format!("Also above {t:.0}%"),
                ),
                None => (
                    format!("For {minutes} minutes."),
                    "Also running hot".to_owned(),
                ),
            };
            let also = if others.is_empty() {
                String::new()
            } else {
                format!(" {also}: {}.", others.join(", "))
            };
            (
                format!("{process} is using {cpu_pct:.0}% CPU"),
                format!("{held}{also} Open Kelvo's Timeline to see what else was running."),
            )
        }
        AlertCause::ThermalState { state } => (
            format!("Thermal state: {}", thermal_label(*state)),
            "macOS is reducing performance to cool the Mac down.".to_owned(),
        ),
    })
}

fn thermal_label(s: ThermalState) -> &'static str {
    match s {
        ThermalState::Nominal => "Nominal",
        ThermalState::Fair => "Fair",
        ThermalState::Serious => "Serious",
        ThermalState::Critical => "Critical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn alert(cause: AlertCause) -> Event {
        Event {
            ts_ms: 400_000,
            start_ms: 100_000,
            processes: vec![],
            detail: EventDetail::Alert {
                rule_id: AlertRule::hot_process().id,
                rule_name: "r".into(),
                cause,
            },
        }
    }

    fn hot(processes: &[&str], rule_id: Uuid) -> Event {
        Event {
            processes: processes.iter().map(|p| (*p).to_owned()).collect(),
            detail: EventDetail::Alert {
                rule_id,
                rule_name: "r".into(),
                cause: AlertCause::ProcessCpu {
                    process: processes[0].into(),
                    cpu_pct: 251.4,
                },
            },
            ..alert(AlertCause::ThermalState {
                state: ThermalState::Serious,
            })
        }
    }

    #[test]
    fn alert_text() {
        let (t, b) = text(&hot(&["ffmpeg"], AlertRule::hot_process().id)).unwrap();
        assert_eq!(t, "ffmpeg is using 251% CPU");
        assert_eq!(
            b,
            "Above 200% for 5 minutes. Open Kelvo's Timeline to see what else was running."
        );
        let (t, _) = text(&alert(AlertCause::ThermalState {
            state: ThermalState::Critical,
        }))
        .unwrap();
        assert_eq!(t, "Thermal state: Critical");
        let other = Event {
            detail: EventDetail::ThermalState {
                from: None,
                to: ThermalState::Serious,
            },
            ..alert(AlertCause::ThermalState {
                state: ThermalState::Serious,
            })
        };
        assert_eq!(text(&other), None);
    }

    /// The threshold comes from the rule that fired, not a literal; a rule this build
    /// does not know names none. Every process of one alert is named.
    #[test]
    fn process_alert_names_the_rules_threshold_and_every_process() {
        let Condition::ProcessCpuAbove { percent_of_core } = AlertRule::hot_process().when else {
            panic!("hot_process is a process CPU rule")
        };
        let (_, b) = text(&hot(&["ffmpeg", "x264"], AlertRule::hot_process().id)).unwrap();
        assert_eq!(
            b,
            format!(
                "Above {percent_of_core:.0}% for 5 minutes. Also above {percent_of_core:.0}%: \
                 x264. Open Kelvo's Timeline to see what else was running."
            )
        );
        let (_, b) = text(&hot(&["ffmpeg"], Uuid::from_u128(99))).unwrap();
        assert!(b.starts_with("For 5 minutes. Open"), "{b}");
    }

    #[test]
    fn alerts_on_text_says_what_will_notify() {
        assert_eq!(alerts_on_text(&[]), None);
        let (t, b) = alerts_on_text(&[AlertRule::thermal_serious()]).unwrap();
        assert_eq!(t, "Alerts are on");
        assert_eq!(
            b,
            "You'll get a notification when the thermal state reaches Serious or worse."
        );
        let (_, b) =
            alerts_on_text(&[AlertRule::hot_process(), AlertRule::thermal_serious()]).unwrap();
        assert_eq!(
            b,
            "You'll get a notification when a process stays above 200% CPU for 5 minutes \
             or when the thermal state reaches Serious or worse."
        );
    }
}
