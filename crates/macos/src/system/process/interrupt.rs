//! Forwards a terminating signal to the subprocess group in flight, so a cancelled command leaves no orphan.

use std::sync::Once;
use std::sync::atomic::{AtomicI32, Ordering};

static ACTIVE_GROUP: AtomicI32 = AtomicI32::new(0);
static INSTALL: Once = Once::new();
const FORWARDED: [i32; 3] = [libc::SIGHUP, libc::SIGINT, libc::SIGTERM];

/// Makes SIGHUP, SIGINT and SIGTERM stop the tracked subprocess group before they end this process. Only
/// the CLI's sandbox path calls it, so a host application that loads the library keeps its own handlers.
pub(super) fn forward_to_active_group() {
    INSTALL.call_once(install);
}

/// Marks one subprocess group as the one a terminating signal must reach. Dropping it clears the mark.
/// One group is tracked at a time, which matches how commands run their subprocesses.
pub(super) struct ActiveGroup;

impl ActiveGroup {
    pub(super) fn track(process_group: i32) -> Self {
        ACTIVE_GROUP.store(process_group, Ordering::SeqCst);
        Self
    }
}

impl Drop for ActiveGroup {
    fn drop(&mut self) {
        ACTIVE_GROUP.store(0, Ordering::SeqCst);
    }
}

fn install() {
    let handler = forward as extern "C" fn(i32) as libc::sighandler_t;
    for signal in FORWARDED {
        let previous = unsafe { libc::signal(signal, handler) };
        if previous == libc::SIG_IGN {
            unsafe { libc::signal(signal, libc::SIG_IGN) };
        }
    }
}

extern "C" fn forward(signal: i32) {
    let group = ACTIVE_GROUP.load(Ordering::SeqCst);
    if group > 0 {
        unsafe { libc::kill(-group, libc::SIGTERM) };
    }
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

#[cfg(test)]
#[path = "interrupt_tests.rs"]
mod tests;
