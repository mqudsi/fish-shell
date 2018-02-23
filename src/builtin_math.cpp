// Implementation of the math builtin.
#include "config.h"  // IWYU pragma: keep

#include <errno.h>
#include <stddef.h>

#include <algorithm>
#include <string>

#include "builtin.h"
#include "builtin_math.h"
#include "common.h"
#include "fallback.h"  // IWYU pragma: keep
#include "io.h"
#include "wgetopt.h"
#include "wutil.h"  // IWYU pragma: keep

#include "muParser.h"
#include "muParserBase.h"
#include "muParserDef.h"

struct math_cmd_opts_t {
    bool print_help = false;
    int scale = 0;
};

// This command is atypical in using the "+" (REQUIRE_ORDER) option for flag parsing.
// This is needed because of the minus, `-`, operator in math expressions.
static const wchar_t *short_options = L"+:hs:";
static const struct woption long_options[] = {{L"scale", required_argument, NULL, 's'},
                                              {L"help", no_argument, NULL, 'h'},
                                              {NULL, 0, NULL, 0}};

static int parse_cmd_opts(math_cmd_opts_t &opts, int *optind,  //!OCLINT(high ncss method)
                          int argc, wchar_t **argv, parser_t &parser, io_streams_t &streams) {
    const wchar_t *cmd = L"math";
    int opt;
    wgetopter_t w;
    while ((opt = w.wgetopt_long(argc, argv, short_options, long_options, NULL)) != -1) {
        switch (opt) {
            case 's': {
                opts.scale = fish_wcstoi(w.woptarg);
                if (errno || opts.scale < 0 || opts.scale > 15) {
                    streams.err.append_format(_(L"%ls: '%ls' is not a valid scale value\n"), cmd,
                                              w.woptarg);
                    return STATUS_INVALID_ARGS;
                }
                break;
            }
            case 'h': {
                opts.print_help = true;
                break;
            }
            case ':': {
                builtin_missing_argument(parser, streams, cmd, argv[w.woptind - 1]);
                return STATUS_INVALID_ARGS;
            }
            case '?': {
                // For most commands this is an error. We ignore it because a math expression
                // can begin with a minus sign.
                *optind = w.woptind - 1;
                return STATUS_CMD_OK;
            }
            default: {
                DIE("unexpected retval from wgetopt_long");
                break;
            }
        }
    }

    *optind = w.woptind;
    return STATUS_CMD_OK;
}

// We read from stdin if we are the second or later process in a pipeline.
static bool math_args_from_stdin(const io_streams_t &streams) {
    return streams.in.is_redirected();
}

/// Get the arguments from stdin.
static const wchar_t *math_get_arg_stdin(wcstring *storage, const io_streams_t &streams) {
    std::string arg;
    for (;;) {
        char ch = '\0';
        long rc = read_blocked(streams.in.fd, &ch, 1);

        if (rc < 0) return NULL;  // failure

        if (rc == 0) {  // EOF
            if (arg.empty()) return NULL;
            break;
        }

        if (ch == '\n') break;  // we're done

        arg += ch;
    }

    *storage = str2wcstring(arg);
    return storage->c_str();
}

/// Return the next argument from argv.
static const wchar_t *math_get_arg_argv(int *argidx, wchar_t **argv) {
    return argv && argv[*argidx] ? argv[(*argidx)++] : NULL;
}

/// Get the arguments from argv or stdin based on the execution context. This mimics how builtin
/// `string` does it.
static const wchar_t *math_get_arg(int *argidx, wchar_t **argv, wcstring *storage,
                                   const io_streams_t &streams) {
    if (math_args_from_stdin(streams)) {
        return math_get_arg_stdin(storage, streams);
    }
    return math_get_arg_argv(argidx, argv);
}

/// Implement integer modulo math operator.
static mu::ValueOrError moduloOperator(double v, double w) { return (int)v % std::max(1, (int)w); };

/// Evaluate math expressions.
static int evaluate_expression(const wchar_t *cmd, parser_t &parser, io_streams_t &streams,
                               math_cmd_opts_t &opts, wcstring &expression) {
    UNUSED(parser);

    // Helper to print an error and return an error code.
    auto printError = [&streams, cmd](const mu::ParserError &err) {
        streams.err.append_format(_(L"%ls: Invalid expression: %ls\n"), cmd, err.GetMsg().c_str());
        return STATUS_CMD_ERROR;
    };

    mu::Parser p;
    // MuParser doesn't implement the modulo operator so we add it ourselves since there are
    // likely users of our old math wrapper around bc that expect it to be available.
    p.DefineOprtChars(L"%");
    mu::OptionalError oerr = p.DefineOprt(L"%", moduloOperator, mu::prINFIX);
    assert(!oerr.has_error() && "Unexpected error defining modulo operator");
    (void)oerr;

    oerr = p.SetExpr(expression);
    if (oerr.has_error()) return printError(oerr.error());

    std::vector<mu::ValueOrError> vs;
    p.Eval(&vs);
    for (const mu::ValueOrError &v : vs) {
        if (v.has_error()) return printError(v.error());
    }
    for (const mu::ValueOrError &v : vs) {
        if (opts.scale == 0) {
            streams.out.append_format(L"%ld\n", static_cast<long>(*v));
        } else {
            streams.out.append_format(L"%.*lf\n", opts.scale, *v);
        }
    }
    return STATUS_CMD_OK;
}

/// The math builtin evaluates math expressions.
int builtin_math(parser_t &parser, io_streams_t &streams, wchar_t **argv) {
    wchar_t *cmd = argv[0];
    int argc = builtin_count_args(argv);
    math_cmd_opts_t opts;
    int optind;

    // Is this really the right way to handle no expression present?
    // if (argc == 0) return STATUS_CMD_OK;

    int retval = parse_cmd_opts(opts, &optind, argc, argv, parser, streams);
    if (retval != STATUS_CMD_OK) return retval;

    if (opts.print_help) {
        builtin_print_help(parser, streams, cmd, streams.out);
        return STATUS_CMD_OK;
    }

    wcstring expression;
    wcstring storage;
    while (const wchar_t *arg = math_get_arg(&optind, argv, &storage, streams)) {
        if (!expression.empty()) expression.push_back(L' ');
        expression.append(arg);
    }

    return evaluate_expression(cmd, parser, streams, opts, expression);
}
