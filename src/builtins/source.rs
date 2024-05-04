use std::os::fd::AsRawFd;

use crate::{
    common::{escape, scoped_push_replacer, FilenameRef},
    fds::wopen_cloexec,
    nix::isatty,
    parser::Block,
    reader::reader_read,
};
use libc::{S_IFMT, S_IFREG};
use nix::{fcntl::OFlag, sys::stat::Mode};

use super::prelude::*;

/// The  source builtin, sometimes called `.`. Evaluates the contents of a file in the current
/// context.
pub fn source(parser: &Parser, streams: &mut IoStreams, args: &mut [&wstr]) -> Result<Option<()>, NonZeroU8> {
    let argc = args.len();
    let opts = HelpOnlyCmdOpts::parse(args, parser, streams)?;
    let cmd = args[0];

    if opts.print_help {
        builtin_print_help(parser, streams, cmd);
        return STATUS_CMD_OK;
    }

    // If we open a file, this ensures we close it.
    let opened_fd;

    // The fd that we read from, either from opened_fd or stdin.
    let fd;
    let func_filename;
    let optind = opts.optind;

    if argc == optind || args[optind] == "-" {
        if streams.stdin_fd < 0 {
            streams
                .err
                .append(wgettext_fmt!("%ls: stdin is closed\n", cmd));
            return Err(STATUS_CMD_ERROR);
        }
        // Either a bare `source` which means to implicitly read from stdin or an explicit `-`.
        if argc == optind && isatty(streams.stdin_fd) {
            // Don't implicitly read from the terminal.
            return Err(STATUS_CMD_ERROR);
        }
        func_filename = FilenameRef::new(L!("-").to_owned());
        fd = streams.stdin_fd;
    } else {
        match wopen_cloexec(args[optind], OFlag::O_RDONLY, Mode::empty()) {
            Ok(fd) => {
                opened_fd = fd;
            }
            Err(_) => {
                let esc = escape(args[optind]);
                streams.err.append(wgettext_fmt!(
                    "%ls: Error encountered while sourcing file '%ls':\n",
                    cmd,
                    &esc
                ));
                builtin_wperror(cmd, streams);
                return Err(STATUS_CMD_ERROR);
            }
        };

        fd = opened_fd.as_raw_fd();
        let mut buf: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut buf) } == -1 {
            let esc = escape(args[optind]);
            streams.err.append(wgettext_fmt!(
                "%ls: Error encountered while sourcing file '%ls':\n",
                cmd,
                &esc
            ));
            return Err(STATUS_CMD_ERROR);
        }

        if buf.st_mode & S_IFMT != S_IFREG {
            let esc = escape(args[optind]);
            streams
                .err
                .append(wgettext_fmt!("%ls: '%ls' is not a file\n", cmd, esc));
            return Err(STATUS_CMD_ERROR);
        }

        func_filename = FilenameRef::new(args[optind].to_owned());
    }

    assert!(fd >= 0, "Should have a valid fd");

    let sb = parser.push_block(Block::source_block(func_filename.clone()));
    let _filename_push = scoped_push_replacer(
        |new_value| std::mem::replace(&mut parser.libdata_mut().current_filename, new_value),
        Some(func_filename.clone()),
    );

    // Construct argv from our null-terminated list.
    // This is slightly subtle. If this is a bare `source` with no args then `argv + optind` already
    // points to the end of argv. Otherwise we want to skip the file name to get to the args if any.
    let mut argv_list: Vec<WString> = vec![];
    let remaining_args = &args[optind + if argc == optind { 0 } else { 1 }..];
    #[allow(clippy::unnecessary_to_owned)]
    for arg in remaining_args.iter().copied() {
        argv_list.push(arg.to_owned());
    }
    parser.vars().set_argv(argv_list);

    let retval = reader_read(parser, fd, streams.io_chain);

    parser.pop_block(sb);

    // Do not close fd after calling reader_read. reader_read automatically closes it before calling
    // eval.
    if retval != 0 {
        let esc = escape(&func_filename);
        streams.err.append(wgettext_fmt!(
            "%ls: Error while reading file '%ls'\n",
            cmd,
            if esc == "-" { L!("<stdin>") } else { &esc }
        ));
        Err(NonZeroU8::new(retval.try_into().unwrap()).unwrap())
    } else {
        parser.get_last_result()
    }
}
