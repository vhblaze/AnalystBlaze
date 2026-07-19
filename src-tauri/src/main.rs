// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--analystblaze-helper-service") {
        analystblaze_desktop_lib::run_privileged_helper_service();
        return;
    }

    analystblaze_desktop_lib::run()
}
