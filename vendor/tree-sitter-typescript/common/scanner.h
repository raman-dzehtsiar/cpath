#include "tree_sitter/parser.h"
#include "tree_sitter/array.h"

#include <wctype.h>

enum TokenType {
    AUTOMATIC_SEMICOLON,
    TEMPLATE_CHARS,
    TERNARY_QMARK,
    HTML_COMMENT,
    LOGICAL_OR,
    ESCAPE_SEQUENCE,
    REGEX_PATTERN,
    JSX_TEXT,
    FUNCTION_SIGNATURE_AUTOMATIC_SEMICOLON,
    ERROR_RECOVERY,
    CPATH_GENERIC_ARGUMENT,
};

static void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

static void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

static bool scan_template_chars(TSLexer *lexer) {
    lexer->result_symbol = TEMPLATE_CHARS;
    for (bool has_content = false;; has_content = true) {
        lexer->mark_end(lexer);
        switch (lexer->lookahead) {
            case '`':
                return has_content;
            case '\0':
                return false;
            case '$':
                advance(lexer);
                if (lexer->lookahead == '{') {
                    return has_content;
                }
                break;
            case '\\':
                return has_content;
            default:
                advance(lexer);
        }
    }
}

static bool scan_whitespace_and_comments(TSLexer *lexer, bool *scanned_comment) {
    for (;;) {
        while (iswspace(lexer->lookahead)) {
            skip(lexer);
        }

        if (lexer->lookahead == '/') {
            skip(lexer);

            if (lexer->lookahead == '/') {
                skip(lexer);
                while (lexer->lookahead != 0 && lexer->lookahead != '\n') {
                    skip(lexer);
                }
                *scanned_comment = true;
            } else if (lexer->lookahead == '*') {
                skip(lexer);
                while (lexer->lookahead != 0) {
                    if (lexer->lookahead == '*') {
                        skip(lexer);
                        if (lexer->lookahead == '/') {
                            skip(lexer);
                            break;
                        }
                    } else {
                        skip(lexer);
                    }
                }
            } else {
                return false;
            }
        } else {
            return true;
        }
    }
}

static bool scan_automatic_semicolon(TSLexer *lexer, const bool *valid_symbols, bool *scanned_comment) {
    lexer->result_symbol = AUTOMATIC_SEMICOLON;
    lexer->mark_end(lexer);

    for (;;) {
        if (lexer->lookahead == 0) {
            return true;
        }
        if (lexer->lookahead == '}') {
            // Automatic semicolon insertion breaks detection of object patterns
            // in a typed context:
            //   type F = ({a}: {a: number}) => number;
            // Therefore, disable automatic semicolons when followed by typing
            do {
                skip(lexer);
            } while (iswspace(lexer->lookahead));
            if (lexer->lookahead == ':') {
                return valid_symbols[LOGICAL_OR]; // Don't return false if we're in a ternary by checking if || is valid
            }
            return true;
        }
        if (!iswspace(lexer->lookahead)) {
            return false;
        }
        if (lexer->lookahead == '\n') {
            break;
        }
        skip(lexer);
    }

    skip(lexer);

    if (!scan_whitespace_and_comments(lexer, scanned_comment)) {
        return false;
    }

    switch (lexer->lookahead) {
        case '`':
        case ',':
        case '.':
        case ';':
        case '*':
        case '%':
        case '>':
        case '<':
        case '=':
        case '?':
        case '^':
        case '|':
        case '&':
        case '/':
        case ':':
            return false;

        case '{':
            if (valid_symbols[FUNCTION_SIGNATURE_AUTOMATIC_SEMICOLON]) {
                return false;
            }
            break;

            // Don't insert a semicolon before a '[' or '(', unless we're parsing
            // a type. Detect whether we're parsing a type or an expression using
            // the validity of a binary operator token.
        case '(':
        case '[':
            if (valid_symbols[LOGICAL_OR]) {
                return false;
            }
            break;

            // Insert a semicolon before `--` and `++`, but not before binary `+` or `-`.
        case '+':
            skip(lexer);
            return lexer->lookahead == '+';
        case '-':
            skip(lexer);
            return lexer->lookahead == '-';

            // Don't insert a semicolon before `!=`, but do insert one before a unary `!`.
        case '!':
            skip(lexer);
            return lexer->lookahead != '=';

            // Don't insert a semicolon before `in` or `instanceof`, but do insert one
            // before an identifier.
        case 'i':
            skip(lexer);

            if (lexer->lookahead != 'n') {
                return true;
            }
            skip(lexer);

            if (!iswalpha(lexer->lookahead)) {
                return false;
            }

            for (unsigned i = 0; i < 8; i++) {
                if (lexer->lookahead != "stanceof"[i]) {
                    return true;
                }
                skip(lexer);
            }

            if (!iswalpha(lexer->lookahead)) {
                return false;
            }
            break;
    }

    return true;
}

static bool scan_ternary_qmark(TSLexer *lexer) {
    for (;;) {
        if (!iswspace(lexer->lookahead)) {
            break;
        }
        skip(lexer);
    }

    if (lexer->lookahead == '?') {
        advance(lexer);

        /* Optional chaining. */
        if (lexer->lookahead == '?' || lexer->lookahead == '.') {
            return false;
        }

        lexer->mark_end(lexer);
        lexer->result_symbol = TERNARY_QMARK;

        /* TypeScript optional arguments contain the ?: sequence, possibly
           with whitespace. */
        for (;;) {
            if (!iswspace(lexer->lookahead)) {
                break;
            }
            advance(lexer);
        }

        if (lexer->lookahead == ':' || lexer->lookahead == ')' || lexer->lookahead == ',') {
            return false;
        }

        if (lexer->lookahead == '.') {
            advance(lexer);
            if (iswdigit(lexer->lookahead)) {
                return true;
            }
            return false;
        }
        return true;
    }
    return false;
}

static bool scan_closing_comment(TSLexer *lexer) {
    while (iswspace(lexer->lookahead) || lexer->lookahead == 0x2028 || lexer->lookahead == 0x2029) {
        skip(lexer);
    }

    const char *comment_start = "<!--";
    const char *comment_end = "-->";

    if (lexer->lookahead == '<') {
        for (unsigned i = 0; i < 4; i++) {
            if (lexer->lookahead != comment_start[i]) {
                return false;
            }
            advance(lexer);
        }
    } else if (lexer->lookahead == '-') {
        for (unsigned i = 0; i < 3; i++) {
            if (lexer->lookahead != comment_end[i]) {
                return false;
            }
            advance(lexer);
        }
    } else {
        return false;
    }

    while (lexer->lookahead != 0 && lexer->lookahead != '\n' && lexer->lookahead != 0x2028 &&
           lexer->lookahead != 0x2029) {
        advance(lexer);
    }

    lexer->result_symbol = HTML_COMMENT;
    lexer->mark_end(lexer);

    return true;
}

static bool scan_jsx_text(TSLexer *lexer) {
    // saw_text will be true if we see any non-whitespace content, or any whitespace content that is not a newline and
    // does not immediately follow a newline.
    bool saw_text = false;
    // at_newline will be true if we are currently at a newline, or if we are at whitespace that is not a newline but
    // immediately follows a newline.
    bool at_newline = false;

    while (lexer->lookahead != 0 && lexer->lookahead != '<' && lexer->lookahead != '>' && lexer->lookahead != '{' &&
           lexer->lookahead != '}' && lexer->lookahead != '&') {
        bool is_wspace = iswspace(lexer->lookahead);
        if (lexer->lookahead == '\n') {
            at_newline = true;
        } else {
            // If at_newline is already true, and we see some whitespace, then it must stay true.
            // Otherwise, it should be false.
            //
            // See the table below to determine the logic for computing `saw_text`.
            //
            // |------------------------------------|
            // | at_newline | is_wspace | saw_text  |
            // |------------|-----------|-----------|
            // | false (0)  | false (0) | true  (1) |
            // | false (0)  | true  (1) | true  (1) |
            // | true  (1)  | false (0) | true  (1) |
            // | true  (1)  | true  (1) | false (0) |
            // |------------------------------------|

            at_newline &= is_wspace;
            if (!at_newline) {
                saw_text = true;
            }
        }

        advance(lexer);
    }

    lexer->result_symbol = JSX_TEXT;
    return saw_text;
}


/* Look ahead without consuming the first '<' of a generic-function type
 * argument. Ordinary shifts retain their original atomic lexer token. */
static bool cpath_skip_lookahead_trivia(TSLexer *lexer) {
    for (;;) {
        while (iswspace(lexer->lookahead)) advance(lexer);
        if (lexer->lookahead != '/') return true;
        advance(lexer);
        if (lexer->lookahead == '/') {
            while (lexer->lookahead && lexer->lookahead != '\n') advance(lexer);
        } else if (lexer->lookahead == '*') {
            advance(lexer);
            bool closed = false;
            while (lexer->lookahead) {
                if (lexer->lookahead == '*') {
                    advance(lexer);
                    if (lexer->lookahead == '/') {
                        advance(lexer);
                        closed = true;
                        break;
                    }
                } else advance(lexer);
            }
            if (!closed) return false;
        } else return false;
    }
}

static bool cpath_skip_quoted_type(TSLexer *lexer) {
    Array(int32_t) delimiters = array_new();
    array_push(&delimiters, lexer->lookahead);
    advance(lexer);
    while (delimiters.size && lexer->lookahead) {
        int32_t end = *array_back(&delimiters);
        int32_t value = lexer->lookahead;
        advance(lexer);
        if (end != '}') {
            if (value == '\\' && lexer->lookahead) advance(lexer);
            else if (value == end) array_pop(&delimiters);
            else if (end == '`' && value == '$' && lexer->lookahead == '{') {
                advance(lexer);
                array_push(&delimiters, '}');
            }
        } else if (value == '}') array_pop(&delimiters);
        else if (value == '{') array_push(&delimiters, '}');
        else if (value == '\'' || value == '"' || value == '`') array_push(&delimiters, value);
        else if (value == '/' && lexer->lookahead == '/') {
            while (lexer->lookahead && lexer->lookahead != '\n') advance(lexer);
        } else if (value == '/' && lexer->lookahead == '*') {
            advance(lexer);
            while (lexer->lookahead) {
                if (lexer->lookahead == '*') {
                    advance(lexer);
                    if (lexer->lookahead == '/') { advance(lexer); break; }
                } else advance(lexer);
            }
        }
    }
    bool complete = delimiters.size == 0;
    array_delete(&delimiters);
    return complete;
}

static bool cpath_identifier_character(int32_t value) {
    return iswalnum(value) || value == '_' || value == '$' || value == '\\' || value >= 128;
}

static bool cpath_scan_generic_argument(TSLexer *lexer) {
    if (lexer->lookahead != '<') return false;
    advance(lexer);
    lexer->mark_end(lexer);
    if (lexer->lookahead != '<') return false;
    advance(lexer);
    unsigned angles = 1, parentheses = 0, brackets = 0, braces = 0;
    bool name_seen = false, constraint = false;
    while (angles && lexer->lookahead) {
        if (!cpath_skip_lookahead_trivia(lexer)) return false;
        int32_t value = lexer->lookahead;
        bool top = angles == 1 && !parentheses && !brackets && !braces;
        if (value == '\'' || value == '"' || value == '`') {
            if (!cpath_skip_quoted_type(lexer)) return false;
            continue;
        }
        if (top && !constraint && cpath_identifier_character(value)) {
            char word[16]; unsigned size = 0;
            while (cpath_identifier_character(lexer->lookahead)) {
                if (size + 1 < sizeof word) word[size++] = (char)lexer->lookahead;
                advance(lexer);
            }
            word[size] = 0;
            if (!name_seen) {
                if (strcmp(word, "const") && strcmp(word, "in") && strcmp(word, "out")) name_seen = true;
            } else if (!strcmp(word, "extends")) constraint = true;
            else return false;
            continue;
        }
        advance(lexer);
        switch (value) {
            case '<':
                if (top && !constraint) return false;
                angles++;
                break;
            case '>': angles--; break;
            case '(' : parentheses++; break;
            case ')' : if (!parentheses) return false; parentheses--; break;
            case '[' : brackets++; break;
            case ']' : if (!brackets) return false; brackets--; break;
            case '{' : braces++; break;
            case '}' : if (!braces) return false; braces--; break;
            case '=':
                if (top) constraint = true;
                if (lexer->lookahead == '>') advance(lexer);
                break;
            case ',': if (top) { name_seen = false; constraint = false; } break;
            case ';': if (top) return false; break;
            case 0: return false;
        }
    }
    if (angles || parentheses || brackets || braces || !cpath_skip_lookahead_trivia(lexer) || lexer->lookahead != '(') return false;
    advance(lexer);
    parentheses = 1;
    while (parentheses && lexer->lookahead) {
        if (!cpath_skip_lookahead_trivia(lexer)) return false;
        int32_t value = lexer->lookahead;
        if (value == '\'' || value == '"' || value == '`') {
            if (!cpath_skip_quoted_type(lexer)) return false;
            continue;
        }
        advance(lexer);
        if (value == '(') parentheses++;
        else if (value == ')') parentheses--;
    }
    if (parentheses || !cpath_skip_lookahead_trivia(lexer) || lexer->lookahead != '=') return false;
    advance(lexer);
    if (lexer->lookahead != '>') return false;
    lexer->result_symbol = CPATH_GENERIC_ARGUMENT;
    return true;
}

static inline bool external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
    if (valid_symbols[TEMPLATE_CHARS]) {
        if (valid_symbols[AUTOMATIC_SEMICOLON]) {
            return false;
        }
        return scan_template_chars(lexer);
    }

    if (valid_symbols[JSX_TEXT] && scan_jsx_text(lexer)) {
        return true;
    }

    if (valid_symbols[AUTOMATIC_SEMICOLON] || valid_symbols[FUNCTION_SIGNATURE_AUTOMATIC_SEMICOLON]) {
        bool scanned_comment = false;
        bool ret = scan_automatic_semicolon(lexer, valid_symbols, &scanned_comment);
        if (!ret && !scanned_comment && valid_symbols[TERNARY_QMARK] && lexer->lookahead == '?') {
            return scan_ternary_qmark(lexer);
        }
        if (!ret && valid_symbols[CPATH_GENERIC_ARGUMENT] && lexer->lookahead == '<') {
            return cpath_scan_generic_argument(lexer);
        }
        return ret;
    }
    if (valid_symbols[CPATH_GENERIC_ARGUMENT]) {
        bool scanned_comment = false;
        if (!scan_whitespace_and_comments(lexer, &scanned_comment)) return false;
        if (lexer->lookahead == '<') return cpath_scan_generic_argument(lexer);
    }
    if (valid_symbols[TERNARY_QMARK]) {
        return scan_ternary_qmark(lexer);
    }

    if (valid_symbols[HTML_COMMENT] && !valid_symbols[LOGICAL_OR] && !valid_symbols[ESCAPE_SEQUENCE] &&
        !valid_symbols[REGEX_PATTERN]) {
        return scan_closing_comment(lexer);
    }

    return false;
}
