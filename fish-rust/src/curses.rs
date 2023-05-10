//! A wrapper around the system's curses/ncurses library, exposing some lower-level functionality
//! that's not directly exposed in any of the popular ncurses crates.
//!
//! In addition to exposing the C library ffi calls, we also shim around some functionality that's
//! only made available via the the ncurses headers to C code via macro magic, such as polyfilling
//! missing capability strings to shoe-in missing support for certain terminal sequences.
//!
//! This is intentionally very bare bones; functionality that we don't need to intercept to provide
//! missing ncurses features is exposed directly to the module's callers via unsafe ffi functions.

use std::ffi::CString;

const OK: i32 = 0;
const ERR: i32 = -1;

extern "C" {
    /// The ncurses `cur_term` TERMINAL pointer.
    static mut cur_term: *const core::ffi::c_void;

    /// setupterm(3) is a low-level call to begin doing any sort of `term.h`/`curses.h` work.
    /// It's called internally by ncurses's `initscr()` and `newterm()`, but the C++ code called
    /// it directly from [`initialize_curses_using_fallbacks()`].
    fn setupterm(
        term: *const libc::c_char,
        filedes: libc::c_int,
        errret: *mut libc::c_int,
    ) -> libc::c_int;

    /// Frees the `cur_term` TERMINAL  pointer.
    fn del_curterm(term: *const core::ffi::c_void);

    /// Checks for the presence of a termcap flag identified by the first two characters of
    /// `id`. The C function returns an integer, but we just reinterpret that as a bool.
    fn tgetflag(id: *const libc::c_char) -> bool;

    fn tgetstr(id: *const libc::c_char, area: *mut *mut libc::c_char) -> *const core::ffi::c_void;
}

pub struct Term {
    pub strings: Strings,
    pub numbers: Numbers,
    pub flags: Flags,
}

impl Term {
    /// Calls the curses `setupterm()` function with either the provided `$TERM` value `term` (or a
    /// null pointer in case `term`) is null for the file descriptor `fd`.
    ///
    /// Note that the `errret` parameter is provided to the function, meaning curses will not write
    /// error output to stderr in case of failure.
    pub fn setup(term: Option<&str>, fd: i32) -> Option<Self> {
        // If cur_term is already initialized for a different $TERM value, calling setupterm() again
        // will leak memory. Call Term::reset() first to free previously allocated resources.
        unsafe {
            Self::reset();
        }

        let result = unsafe {
            let mut err = 0;
            if let Some(term) = term {
                let term = CString::new(term).unwrap();
                setupterm(term.as_ptr(), fd, &mut err)
            } else {
                setupterm(core::ptr::null(), fd, &mut err)
            }
        };

        if result == self::OK {
            Some(Term::new())
        } else {
            None
        }
    }

    /// Internal constructor function. Like `Default` but only usable from within the module.
    fn new() -> Self {
        Term {
            strings: Strings::default(),
            numbers: Numbers {},
            flags: Flags {},
        }
    }

    /// Resets the curses `cur_term` TERMINAL pointer. Any previous term objects are invalidated!
    pub unsafe fn reset() {
        if Self::is_initialized() {
            del_curterm(cur_term);
            cur_term = core::ptr::null();
        }
    }

    /// Whether or not the curses library has been initialized.
    pub fn is_initialized() -> bool {
        unsafe { !cur_term.is_null() }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Capability {
    /// The two-char termcap code for the capability, followed by a nul.
    code: [u8; 3],
}

impl Capability {
    const fn new(code: &str) -> Capability {
        let code = code.as_bytes();
        Capability {
            code: [code[0], code[1], b'\0'],
        }
    }

    /// The nul-terminated termcap id of the capability.
    pub const fn as_ptr(&self) -> *const libc::c_char {
        self.code.as_ptr().cast()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StringCap(Capability);
impl StringCap {
    const fn new(code: &str) -> Self {
        StringCap(Capability::new(code))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Number(Capability);
impl Number {
    const fn new(code: &str) -> Self {
        Number(Capability::new(code))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Flag(Capability);
impl Flag {
    const fn new(code: &str) -> Self {
        Flag(Capability::new(code))
    }
}

/// Capabilities representing strings. Only the ones we use are included.
#[derive(Default)]
pub struct Strings {
    /// The term strings the caller has overridden.
    values: Vec<(StringCap, String)>,
}

impl Strings {
    pub const ENTER_ITALICS_MODE: StringCap = StringCap::new("ZH");
    pub const EXIT_ITALICS_MODE: StringCap = StringCap::new("ZR");
    pub const ENTER_DIM_MODE: StringCap = StringCap::new("mh");

    /// Queries the termcap/terminfo database for the value of a string Capability.
    pub fn get(&mut self, id: StringCap) -> Option<&str> {
        match self.values.binary_search_by(|entry| entry.0.cmp(&id)) {
            Ok(idx) => Some(&self.values[idx].1),
            Err(idx) => {
                let mut result = vec![b'\0'; 100];
                unsafe {
                    let mut area = result.as_mut_ptr() as *mut libc::c_char;
                    let area = std::ptr::addr_of_mut!(area);
                    if tgetstr(id.0.as_ptr(), area).is_null() {
                        return None;
                    }
                }
                self.values
                    .insert(idx, (id, String::from_utf8(result).unwrap()));
                Some(&self.values[idx].1)
            }
        }
    }

    /// Overrides the string value of `capability` for the current terminal.
    pub fn set(&mut self, id: StringCap, value: String) {
        match self.values.binary_search_by(|entry| entry.0.cmp(&id)) {
            Ok(idx) => self.values[idx] = (id, value),
            Err(idx) => self.values.insert(idx, (id, value)),
        }
    }
}

/// Capabilities representing flags. Only the ones we use are included.
#[derive(Default)]
pub struct Flags {}

impl Flags {
    pub const EAT_NEWLINE_GLITCH: Flag = Flag::new("xn");

    /// Queries the termcap/terminfo database for the presence of a Capability.
    pub fn get(&self, id: Flag) -> bool {
        unsafe { tgetflag(id.0.as_ptr()) }
    }
}

/// Capabilities representing numbers. Only the ones we use are included.
#[derive(Default)]
pub struct Numbers {}

impl Numbers {}
