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
use crate::common::ToCString;
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::sync::Arc;

/// The [`Term`] singleton, providing a façade around the system curses library. Initialized via a
/// successful call to [`setup()`] and surfaced to the outside world via [`term()`].
///
/// It isn't guaranteed that fish will ever be able to successfully call `setup()`, so this must
/// remain an `Option` instead of returning `Term` by default and just panicking if [`term()`] was
/// called before `setup()`.
///
/// We can't use Lazy here to avoid the `static mut` because a `Lazy` can only be init once and we
/// need to support re-initialization (via [`setup()`]) if `$TERM` changes.
///
/// In order for this to be truly safe, we can't have any [`Term`] function hand back results that
/// borrow from the `Term` instance. If we do end up doing that it isn't the end of the world but
/// we'd have to make [`setup()`] `unsafe` (as calling it would invalidate any existing term
/// references).
pub static mut TERM: Option<Term> = None;

/// Returns a reference to the global [`Term`] singleton or `None` if not preceded by a successful
/// call to [`curses::setup()`].
pub fn term() -> Option<&'static Term> {
    unsafe { TERM.as_ref() }
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
        /// `id`.
        pub fn tgetflag(id: *const libc::c_char) -> libc::c_int;

        /// Checks for the presence and value of a number capability in the termcap/termconf
        /// database. A return value of `-1` indicates not found.
        pub fn tgetnum(id: *const libc::c_char) -> libc::c_int;

        pub fn tgetstr(
            id: *const libc::c_char,
            area: *mut *mut libc::c_char,
        ) -> *const libc::c_char;
    }
}

/// The safe wrapper around curses functionality, initialized by a successful call to [`setup()`]
/// and obtained thereafter by calls to [`term()`].
///
/// An extant `Term` instance means the curses `TERMINAL *cur_term` pointer is non-null. Any
/// functionality that is normally performed using `cur_term` should be done via `Term` instead.
pub struct Term {
    /// A list of cached string values corresponding to known curses string capabilities. Individual
    /// values may be overridden with [`Term::set()`].
    ///
    /// The list is prepopulated on initialization in [`Term::new()`], as such the `Vec` itself is
    /// never manipulated - only potentially the values within its individual cells.
    strings: Vec<RefCell<Option<Arc<CString>>>>,
}

impl Term {
    /// Looks up support for [`Capability`] `capability` in the termcap/terminfo database via the
    /// curses library.
    pub fn get<C: Capability>(&self, capability: C) -> C::Result {
        capability.lookup(self)
    }

    /// Overrides the string value of `capability` for the current terminal.
    pub fn set<S>(&self, id: StringCap, value: Option<S>)
    where
        S: ToCString,
    {
        let value = value.map(|s| Arc::new(s.to_cstring()));
        *self.strings[id.idx()].borrow_mut() = value;
    }

    /// Initialize a new `Term` instance, prepopulating the values of all the curses string
    /// capabilities we care about in the process.
    fn new() -> Self {
        let mut strings = vec![RefCell::new(None); STRING_CAPS.len()];
        for cap in STRING_CAPS {
            strings[cap.idx()] = cap
                .sys_lookup()
                // Convert the Option<CString> to a RefCell<Option<Arc<CString>>>
                .map(Arc::new)
                .into();
        }

        Term { strings }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
// Clippy deduces the color of all the cows in Scotland from the first three entries we happen to
// have here.
#[allow(clippy::enum_variant_names)]
enum StringCapIdx {
    EnterItalicsMode,
    ExitItalicsMode,
    EnterDimMode,
}

// It's preferred **but not required** for the order of `StringCap`s here to match the order of
// `StringCapIdx` entries, as we always use the associated `StringCapIdx::idx()` to get the index.
const STRING_CAPS: [StringCap; 3] = [
    StringCap::new("ZH", StringCapIdx::EnterItalicsMode),
    StringCap::new("ZR", StringCapIdx::ExitItalicsMode),
    StringCap::new("mh", StringCapIdx::EnterDimMode),
];

// String capabilities
pub const ENTER_ITALICS_MODE: StringCap = STRING_CAPS[StringCapIdx::EnterItalicsMode.idx()];
pub const EXIT_ITALICS_MODE: StringCap = STRING_CAPS[StringCapIdx::ExitItalicsMode.idx()];
pub const ENTER_DIM_MODE: StringCap = STRING_CAPS[StringCapIdx::EnterDimMode.idx()];

// Number capabilities
pub const MAX_COLORS: Number = Number::new("Co");

// Flag capabilities
pub const EAT_NEWLINE_GLITCH: Flag = Flag::new("xn");

pub trait Capability {
    type Result: Sized;
    fn lookup(&self, term: &Term) -> Self::Result;
}

impl StringCap {
    /// Looks up a curses string capabality and clones the result into an owned buffer if it was
    /// found. This is only called upon initialization of a [`Term`] instance in [`Term::new()`].
    fn sys_lookup(&self) -> Option<CString> {
        unsafe {
            const NULL: *const i8 = core::ptr::null();
            match sys::tgetstr(self.code.as_ptr(), core::ptr::null_mut()) {
                NULL => None,
                // termcap spec says nul is not allowed in terminal sequences and must be encoded;
                // so the terminating NUL is the end of the string.
                result => Some(CStr::from_ptr(result).to_owned()),
            }
        }
    }
}

impl Capability for StringCap {
    type Result = Option<Arc<CString>>;

    /// We prepopulate all string capabilities at startup to never need to resize the vector, so we
    /// never have to use [`StringCap::sys_lookup()`] here.
    fn lookup(&self, term: &Term) -> Self::Result {
        let idx = self.idx as u8 as usize;
        term.strings[idx].borrow().clone()
    }
}

impl Capability for Number {
    type Result = Option<i32>;

    fn lookup(&self, _: &Term) -> Self::Result {
        unsafe {
            match tgetnum(self.0.as_ptr()) {
                -1 => None,
                n => Some(n),
            }
        }
    }
}

impl Capability for Flag {
    type Result = bool;

    fn lookup(&self, _: &Term) -> Self::Result {
        unsafe { tgetflag(self.0.as_ptr()) != 0 }
    }
}

impl StringCapIdx {
    const fn idx(&self) -> usize {
        *self as u8 as usize
    }
}

/// Calls the curses `setupterm()` function with the provided `$TERM` value `term` (or a null
/// pointer in case `term` is null) for the file descriptor `fd`. Returns a reference to the newly
/// initialized [`Term`] singleton on success or `None` if this failed.
///
/// Note that the `errret` parameter is provided to the function, meaning curses will not write
/// error output to stderr in case of failure.
///
/// Any existing references from `curses::term()` will be invalidated by this call!
pub fn setup(term: Option<&CStr>, fd: i32) -> Option<&'static Term> {
    let result = unsafe {
        // If cur_term is already initialized for a different $TERM value, calling setupterm() again
        // will leak memory. Call reset() first to free previously allocated resources.
        reset();

        let mut err = 0;
        if let Some(term) = term {
            sys::setupterm(term.as_ptr(), fd, &mut err)
        } else {
            sys::setupterm(core::ptr::null(), fd, &mut err)
        }
    };

    unsafe {
        if result == sys::OK {
            TERM = Some(Term::new());
        } else {
            TERM = None;
        }
    }
    self::term()
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StringCap {
    code: Code,
    idx: StringCapIdx,
}
impl StringCap {
    const fn new(code: &str, idx: StringCapIdx) -> Self {
        StringCap {
            code: Code::new(code),
            idx,
        }
    }

    const fn idx(&self) -> usize {
        self.idx.idx()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Number(Code);
impl Number {
    const fn new(code: &str) -> Self {
        Number(Code::new(code))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Flag(Code);
impl Flag {
    const fn new(code: &str) -> Self {
        Flag(Code::new(code))
    }
}
