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
use crate::common::Projection;
use std::ffi::CString;
use std::sync::Mutex;

pub static mut TERM: Mutex<Option<Term>> = Mutex::new(None);

/// Returns a mutable reference to the global [`Term`] singleton. Locks if another thread has an
/// outstanding reference.
///
/// Panics on deref if [`setup()`](self::setup()) hasn't been called successfully.
pub fn term() -> impl std::ops::DerefMut<Target = Term> {
    unsafe {
        let guard = TERM.lock().expect("Mutex poisoned!");

        Projection::new(
            guard,
            |guard| guard.as_ref().expect("TERM hasn't been initialized!"),
            |guard| guard.as_mut().expect("TERM hasn't been initialized!"),
        )
    }
}

const OK: i32 = 0;
const ERR: i32 = -1;

/// Private module exposing system curses ffi.
mod sys {
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
        ) -> *const core::ffi::c_void;
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
    overrides: Vec<(StringCap, String)>,
}

impl Term {
    /// Internal constructor function. Like `Default` but only usable from within the module.
    fn new() -> Self {
        Term {
            overrides: Vec::new(),
        }
    }

    /// Looks up support for [`Capability`] `capability` in the termcap/terminfo database via the
    /// curses library.
    pub fn get<'a, C: Capability<'a>>(&'a mut self, capability: C) -> C::Result {
        capability.lookup(self)
    }

    /// Overrides the string value of `capability` for the current terminal.
    pub fn set(&mut self, id: StringCap, value: String) {
        match self.overrides.binary_search_by(|entry| entry.0.cmp(&id)) {
            Ok(idx) => self.overrides[idx] = (id, value),
            Err(idx) => self.overrides.insert(idx, (id, value)),
        }
    }
}

enum Value<'a> {
    String(&'a str),
    Bool(bool),
    Number(i32),
}

pub trait Capability<'a> {
    type Result: Sized + 'a;
    fn lookup(&self, term: &'a mut Term) -> Self::Result;
}

impl<'a> Capability<'a> for StringCap {
    type Result = Option<&'a str>;

    fn lookup(&self, term: &'a mut Term) -> Self::Result {
        let id = self.0;
        match term.overrides.binary_search_by(|entry| entry.0.cmp(self)) {
            Ok(idx) => Some(&term.overrides[idx].1),
            Err(idx) => {
                let mut result = vec![b'\0'; 100];
                unsafe {
                    let mut area = result.as_mut_ptr() as *mut libc::c_char;
                    let area = std::ptr::addr_of_mut!(area);
                    if sys::tgetstr(id.as_ptr(), area).is_null() {
                        return None;
                    }
                }
                term.overrides
                    .insert(idx, (*self, String::from_utf8(result).unwrap()));
                Some(&term.overrides[idx].1)
            }
        }
    }
}

impl<'a> Capability<'a> for Number {
    type Result = Option<i32>;

    fn lookup(&self, _: &'a mut Term) -> Self::Result {
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

    fn lookup(&self, _: &'a mut Term) -> Self::Result {
        unsafe { tgetflag(self.0.as_ptr()) }
    }
}

/// Calls the curses `setupterm()` function with the provided `$TERM` value `term` (or a null
/// pointer in case `term` is null) for the file descriptor `fd`.
///
/// Note that the `errret` parameter is provided to the function, meaning curses will not write
/// error output to stderr in case of failure.
pub fn setup(term: Option<&str>, fd: i32) -> bool {
    // If cur_term is already initialized for a different $TERM value, calling setupterm() again
    // will leak memory. Call reset() first to free previously allocated resources.
    unsafe {
        reset();

        let result = {
            let mut err = 0;
            if let Some(term) = term {
                let term = CString::new(term).unwrap();
                sys::setupterm(term.as_ptr(), fd, &mut err)
            } else {
                sys::setupterm(core::ptr::null(), fd, &mut err)
            }
        };

        if result == self::OK {
            *TERM.lock().expect("Mutex poisoned!") = Some(Term::new());
            true
        } else {
            *TERM.lock().expect("Mutex poisoned!") = None;
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

/// Capabilities representing strings. Only the ones we use are included.
#[derive(Default)]
pub struct Strings {
    /// The term strings the caller has overridden.
    values: Vec<(StringCap, String)>,
}

impl Strings {}

/// Capabilities representing flags. Only the ones we use are included.
#[derive(Default)]
pub struct Flags {}

impl Flags {
    /// Queries the termcap/terminfo database for the presence of a Capability.
    pub fn get(&self, id: Flag) -> bool {
        unsafe { sys::tgetflag(id.0.as_ptr()) }
    }
}

/// Capabilities representing numbers. Only the ones we use are included.
#[derive(Default)]
pub struct Numbers {}

impl Numbers {}
