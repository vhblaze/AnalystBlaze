use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prevents spawned console programs (ping, netsh, powershell, sc.exe, ...)
/// from flashing a visible CMD window on top of the app.
pub trait CommandExt {
    fn no_window(&mut self) -> &mut Command;
}

impl CommandExt for Command {
    #[cfg(windows)]
    fn no_window(&mut self) -> &mut Command {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(windows))]
    fn no_window(&mut self) -> &mut Command {
        self
    }
}
