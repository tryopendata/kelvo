//! Window-scoped commands owned by the popover shell.
use tauri::Manager;

/// Only the popover shell may report its intrinsic content size.
#[tauri::command]
#[specta::specta]
pub fn report_popover_content_height(window: tauri::WebviewWindow, height: f64) {
    let Some(height) = accepted_height(window.label(), height) else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        let app = window.app_handle().clone();
        let handle = app.clone();
        if let Err(e) =
            app.run_on_main_thread(move || crate::popover::report_content_height(&handle, height))
        {
            tracing::debug!("popover content size: {e}");
        }
    }
}

fn accepted_height(window: &str, height: f64) -> Option<f64> {
    (window == super::LABEL && height.is_finite() && (1.0..=100_000.0).contains(&height))
        .then(|| height.ceil().min(super::HEIGHT_PT))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_popover_reports_with_finite_positive_bounds_can_change_size() {
        for window in ["dashboard", "onboarding", "other"] {
            assert_eq!(super::accepted_height(window, 400.0), None);
        }
        for height in [f64::NAN, f64::INFINITY, -1.0, 0.0, 100_001.0] {
            assert_eq!(super::accepted_height("popover", height), None);
        }
        assert_eq!(super::accepted_height("popover", 418.2), Some(419.0));
        assert_eq!(super::accepted_height("popover", 900.0), Some(680.0));
    }
}
