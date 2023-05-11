//! A wrapper around the system's curses/ncurses library, exposing some lower-level functionality
//! that's not directly exposed in any of the popular ncurses crates.
//!
//! In addition to exposing the C library ffi calls, we also shim around some functionality that's
//! only made available via the the ncurses headers to C code via macro magic, such as polyfilling
//! missing capability strings to shoe-in missing support for certain terminal sequences.
//!
//! This is intentionally very bare bones and only implements the subset of curses functionality
//! used by fish

use self::sys::*;
use crate::supercow::ArcCow;
use once_cell::sync::Lazy;
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// This is `static mut` because a `Lazy` can only be init once but we may need to re-init the Term
// value after a second+ call to curses::setup() in response to environment changes.
pub static mut TERM: Lazy<Term> = Lazy::new(|| panic!("TERM not yet initialized!"));

/// Returns a mutable reference to the global [`Term`] singleton.
///
/// Panics on deref if [`curses::setup()`](self::setup()) hasn't been called successfully.
pub fn term() -> &'static Term {
    unsafe { &*TERM }
}

/// Private module exposing system curses ffi.
mod sys {
    pub const OK: i32 = 0;
    pub const ERR: i32 = -1;

    extern "C" {
        /// The ncurses `cur_term` TERMINAL pointer.
        pub static mut cur_term: *const core::ffi::c_void;

        /// setupterm(3) is a low-level call to begin doing any sort of `term.h`/`curses.h` work.
        /// It's called internally by ncurses's `initscr()` and `newterm()`, but the C++ code called
        /// it directly from [`initialize_curses_using_fallbacks()`].
        pub fn setupterm(
            term: *const libc::c_char,
            filedes: libc::c_int,
            errret: *mut libc::c_int,
        ) -> libc::c_int;

        /// Frees the `cur_term` TERMINAL  pointer.
        pub fn del_curterm(term: *const core::ffi::c_void);

        /// Checks for the presence of a termcap flag identified by the first two characters of
        /// `id`. The C function returns an integer, but we just reinterpret that as a bool.
        pub fn tgetflag(id: *const libc::c_char) -> bool;

        /// Checks for the presence and value of a number capability in the termcap/termconf
        /// database. A return value of `-1` indicates not found.
        pub fn tgetnum(id: *const libc::c_char) -> libc::c_int;

        pub fn tgetstr(
            id: *const libc::c_char,
            area: *mut *mut libc::c_char,
        ) -> *const libc::c_char;
    }
}

// String capabilities
pub const ENTER_ITALICS_MODE: StringCap = StringCap::new("ZH");
pub const EXIT_ITALICS_MODE: StringCap = StringCap::new("ZR");
pub const ENTER_DIM_MODE: StringCap = StringCap::new("mh");

// Number capabilities
pub const MAX_COLORS: Number = Number::new("Co");

// Flag capabilities
pub const EAT_NEWLINE_GLITCH: Flag = Flag::new("xn");

pub struct Term {
    has_overrides: AtomicBool,
    overrides: Mutex<Vec<(StringCap, Arc<CString>)>>,
}

impl Term {
    /// Internal constructor function. Like `Default` but only usable from within the module.
    fn new() -> Self {
        Term {
            has_overrides: AtomicBool::new(false),
            overrides: Mutex::new(Vec::new()),
        }
    }

    /// Looks up support for [`Capability`] `capability` in the termcap/terminfo database via the
    /// curses library.
    pub fn get<'a, C: Capability<'a>>(&'a self, capability: C) -> C::Result {
        capability.lookup(self)
    }

    /// Overrides the string value of `capability` for the current terminal.
    pub fn set(&self, id: StringCap, value: &str) {
        let value = CString::new(value).unwrap().into();
        let mut overrides = self.overrides.lock().expect("Mutex poisoned!");
        match overrides.binary_search_by(|entry| entry.0.cmp(&id)) {
            Ok(idx) => overrides[idx] = (id, value),
            Err(idx) => overrides.insert(idx, (id, value)),
        }
        self.has_overrides.store(true, Ordering::Relaxed);
    }
}

pub trait Capability<'a> {
    type Result: Sized + 'a;
    fn lookup(&self, term: &'a Term) -> Self::Result;
}

impl StringCap {
    fn sys_lookup<'a>(&self) -> Option<ArcCow<'a, CStr, CString>> {
        let result = unsafe {
            const NULL: *const i8 = core::ptr::null();
            match sys::tgetstr(self.0.as_ptr(), core::ptr::null_mut()) {
                NULL => return None,
                // termcap spec says nul is not allowed in terminal sequences and must be encoded,
                // so we can safely unwrap here.
                result => CStr::from_ptr(result),
            }
        };
        Some(ArcCow::Borrowed(result))
    }
}

impl<'a> Capability<'a> for StringCap {
    type Result = Option<ArcCow<'a, CStr, CString>>;

    fn lookup(&self, term: &'a Term) -> Self::Result {
        if term.has_overrides.load(Ordering::Relaxed) {
            let overrides = term.overrides.lock().expect("Mutex poisoned!");
            if let Ok(idx) = overrides.binary_search_by(|entry| entry.0.cmp(self)) {
                return Some(ArcCow::Shared(Arc::clone(&overrides[idx].1)));
            }
        }

        self.sys_lookup()
    }
}

impl<'a> Capability<'a> for Number {
    type Result = Option<i32>;

    fn lookup(&self, _: &'a Term) -> Self::Result {
        unsafe {
            match tgetnum(self.0.as_ptr()) {
                -1 => None,
                n => Some(n),
            }
        }
    }
}

impl<'a> Capability<'a> for Flag {
    type Result = bool;

    fn lookup(&self, _: &'a Term) -> Self::Result {
        unsafe { tgetflag(self.0.as_ptr()) }
    }
}

/// Calls the curses `setupterm()` function with the provided `$TERM` value `term` (or a null
/// pointer in case `term` is null) for the file descriptor `fd`.
///
/// Note that the `errret` parameter is provided to the function, meaning curses will not write
/// error output to stderr in case of failure.
///
/// Any existing references from `curses::term()` will be invalidated by this call!
pub fn setup(term: Option<&str>, fd: i32) -> bool {
    let result = unsafe {
        // If cur_term is already initialized for a different $TERM value, calling setupterm() again
        // will leak memory. Call reset() first to free previously allocated resources.
        reset();

        let mut err = 0;
        if let Some(term) = term {
            let term = CString::new(term).unwrap();
            sys::setupterm(term.as_ptr(), fd, &mut err)
        } else {
            sys::setupterm(core::ptr::null(), fd, &mut err)
        }
    };

    unsafe {
        if result == sys::OK {
            TERM = Lazy::new(|| Term::new());
            true
        } else {
            TERM = Lazy::new(|| panic!("TERM has not yet been initialized!"));
            false
        }
    }
}

/// Whether or not the curses library has been initialized.
pub fn is_initialized() -> bool {
    unsafe { !sys::cur_term.is_null() }
}

/// Resets the curses `cur_term` TERMINAL pointer. Any previous term objects are invalidated!
pub unsafe fn reset() {
    if is_initialized() {
        sys::del_curterm(cur_term);
        sys::cur_term = core::ptr::null();
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Code {
    /// The two-char termcap code for the capability, followed by a nul.
    code: [u8; 3],
}

impl Code {
    /// `code` is the two-digit termcap code. See termcap(5) for a reference.
    ///
    /// Panics if anything other than a two-ascii-character `code` is passed into the function. It
    /// would take a hard-coded `[u8; 2]` parameter but that is less ergonomic. Since all our
    /// termcap `Code`s are compile-time constants, the panic is a compile-time error, meaning
    /// there's no harm to going this more ergonomic route.
    const fn new(code: &str) -> Code {
        let code = code.as_bytes();
        if code.len() != 2 {
            panic!("Invalid termcap code provided!");
        }
        Code {
            code: [code[0], code[1], b'\0'],
        }
    }

    /// The nul-terminated termcap id of the capability.
    pub const fn as_ptr(&self) -> *const libc::c_char {
        self.code.as_ptr().cast()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StringCap(Code);
impl StringCap {
    const fn new(code: &str) -> Self {
        StringCap(Code::new(code))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Number(Code);
impl Number {
    const fn new(code: &str) -> Self {
        Number(Code::new(code))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Flag(Code);
impl Flag {
    const fn new(code: &str) -> Self {
        Flag(Code::new(code))
    }
}
