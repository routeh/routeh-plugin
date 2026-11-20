//! What ends a service: a flag its caller sets, or SIGTERM and SIGINT.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by a signal handler: a handler can do no more than store.
static SIGNALLED: AtomicBool = AtomicBool::new(false);

/// Tells [`serve`](crate::serve) to stop. Clones share it.
#[derive(Clone, Debug, Default)]
pub struct Stop {
    flag: Arc<AtomicBool>,
    signals: bool,
}

impl Stop {
    /// Stopped only by [`Stop::stop`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Stopped also by SIGTERM and SIGINT, whose handlers it installs for
    /// the whole process. In ujail the service is PID 1 of its namespace,
    /// where a signal it does not handle is ignored: procd's SIGTERM would
    /// do nothing until its SIGKILL.
    pub fn on_signals() -> Self {
        extern "C" fn on_signal(_: libc::c_int) {
            SIGNALLED.store(true, Ordering::Relaxed);
        }
        let handler = on_signal as extern "C" fn(libc::c_int);
        // SAFETY: the handler only stores to an atomic.
        unsafe {
            libc::signal(libc::SIGTERM, handler as libc::sighandler_t);
            libc::signal(libc::SIGINT, handler as libc::sighandler_t);
        }
        Self {
            signals: true,
            ..Self::default()
        }
    }

    pub fn stop(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    pub fn is_stopped(&self) -> bool {
        self.flag.load(Ordering::Relaxed) || (self.signals && SIGNALLED.load(Ordering::Relaxed))
    }
}
