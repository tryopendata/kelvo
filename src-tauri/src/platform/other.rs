//! Stand-ins for platforms the app shell does not ship on.

pub fn set_launch_at_login(_enabled: bool) -> Result<(), String> {
    Ok(())
}

pub fn launch_at_login_registered() -> Option<bool> {
    None
}

pub fn reduce_transparency() -> bool {
    false
}

pub fn observe_reduce_transparency(_on_change: impl Fn(bool) + 'static) {}

pub fn launched_at_login() -> bool {
    false
}
