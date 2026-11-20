#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let mut args = std::env::args_os().skip(1);
    if let Some(option) = args.next() {
        if option != "--hardware-check" {
            return std::process::ExitCode::FAILURE;
        }
        let Some(path) = args.next() else {
            return std::process::ExitCode::FAILURE;
        };
        let writes = args
            .next()
            .is_some_and(|option| option == "--verify-writes");
        return match openconnex_core::hardware_check(std::path::Path::new(&path), writes) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(_) => std::process::ExitCode::FAILURE,
        };
    }
    match openconnex_core::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This build needs the Windows Bluetooth transport.");
}
