#![cfg_attr(test, allow(clippy::too_many_lines))]
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    kelvo_lib::run()
}
