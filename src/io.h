#ifndef FISH_IO_H
#define FISH_IO_H

#include <memory>

enum stream_flags : int32_t {
    STREAM_TTY = 1 << 0,
    STREAM_REDIRECTED = 1 << 1,
    STREAM_OPENED = 1 << 2,
    STREAM_FILE = 1 << 3,
    STREAM_NULL = 1 << 4,
    STREAM_PIPE = 1 << 5,
    STREAM_WRITTEN_TO = 1 << 6,
};

/// io_stream_t is a wrapper around an fd (and optionally an associated FILE*) that
/// provides metainfo about the underlying stream.
class io_stream_t {
private:
    stream_flags flags;

public:
    /// Underlying file descriptor. Every stream is backed by one (unless closed)
    int fd;
    FILE *fp;

    bool is_tty() const {
        return (flags & STREAM_TTY) != 0;
    }

    bool is_redirected() const {
        return (flags & STREAM_REDIRECTED) != 0;
    }

    void open() {
        if (!(flags & STREAM_OPENED)) {
            fp = fdopen(fd, "rw");
        }
    }

    void append(const wcstring &str) {
        return append_format(L"%s", str.c_str());
    }

    void append(const wchar_t *start, int len) {
        return append(wcstring(start, len));
    }

    void append(const wchar_t c) {
        return append_format(L"%c", c);
    }

    void append_format(const wcstring &format, ...) {
        open();
        flags = (stream_flags)(flags & STREAM_WRITTEN_TO);

        va_list args;
        va_start(args, format);
        vfprintf(fp, wcs2string(format).c_str(), args);
        va_end(args);
    }

    void close() {
        if ((flags & STREAM_OPENED)) {
            fclose(fp);
            fp = nullptr;
            flags = (stream_flags)(flags & ~STREAM_OPENED);
        }
    }

    bool empty() {
        return (flags & STREAM_WRITTEN_TO) != 0;
    }
};

/// io_streams_t is a grouping of the three streams that must be defined
/// for each process.
struct io_streams_t {
    io_stream_t in;
    io_stream_t out;
    io_stream_t err;
};

// io_chain_t should only be used to keep track of all active streams and
// should only be used internally in exec.cpp. Defined here while replacing
// old code.
typedef std::vector<io_stream_t> io_chain_t;

#endif
