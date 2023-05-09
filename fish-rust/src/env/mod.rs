mod env_ffi;
pub mod environment;
mod environment_impl;
pub mod var;

pub use env_ffi::EnvStackSetResult;
pub use environment::*;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicUsize};
pub use var::*;

/// Limit `read` to 100 MiB (bytes, not wide chars) by default. This can be overriden with the
/// `fish_read_limit` variable.
pub const DEFAULT_READ_BYTE_LIMIT: usize = 100 * 1024 * 1024;

/// The actual `read` limit in effect, defaulting to [`DEFAULT_READ_BYTE_LIMIT`] but overridable
/// with `$fish_read_limit`.
#[no_mangle]
pub static READ_BYTE_LIMIT: AtomicUsize = AtomicUsize::new(DEFAULT_READ_BYTE_LIMIT);

/// The curses `cur_term` TERMINAL pointer has been set up.
#[no_mangle]
pub static CURSES_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Does the terminal have the "eat new line" glitch.
#[no_mangle]
pub static TERM_HAS_XN: AtomicBool = AtomicBool::new(false);

mod ffi {
    extern "C" {
        pub fn setenv_lock(
            name: *const libc::c_char,
            value: *const libc::c_char,
            overwrite: libc::c_int,
        );
        pub fn unsetenv_lock(name: *const libc::c_char);
    }
}

pub fn setenv_lock(name: &str, value: &str, overwrite: bool) {
    unsafe {
        let name = CString::new(name).unwrap();
        let value = CString::new(value).unwrap();
        self::ffi::setenv_lock(name.as_ptr(), value.as_ptr(), libc::c_int::from(overwrite));
    }
}

pub fn unsetenv_lock(name: &str) {
    unsafe {
        let name = CString::new(name).unwrap();
        self::ffi::unsetenv_lock(name.as_ptr());
    }
}
