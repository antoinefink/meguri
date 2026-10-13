"""Per-language lexing into the token streams the channels fingerprint.

A file becomes:
- struct tokens: keywords and punctuation kept, identifiers -> "I", numbers -> "N",
  string literals -> "S" (namespaced by language family, so struct k-grams only
  match within one family);
- lex tokens: identifier words split from camel and snake case, plus the words and
  numbers inside string literals and number literals, lowercased; language
  keywords, stopwords and interop vocabulary are dropped;
- comment blocks: lowercased words of each run of adjacent comments;
- text sequences: word sequences used by the `text` channel (String Catalog values,
  Fluent messages, CSS, and string literals on the corpus side).

Every token carries the 1-based line it starts on.
"""

from __future__ import annotations

import bisect
import json
import re
from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# Languages

EXTENSION_LANGUAGE = {
    ".rs": "rust",
    ".py": "python",
    ".js": "js",
    ".mjs": "js",
    ".cjs": "js",
    ".jsx": "js",
    ".ts": "js",
    ".tsx": "js",
    ".svelte": "svelte",
    ".swift": "swift",
    ".css": "css",
    ".scss": "scss",
    ".html": "html",
    ".htm": "html",
    ".sql": "sql",
    ".proto": "proto",
    ".ftl": "ftl",
    ".xcstrings": "xcstrings",
}

# Struct tokens are namespaced by family: TypeScript, JavaScript and Svelte
# scripts share one family; the others are their own.
STRUCT_FAMILY = {
    "rust": "rust",
    "python": "python",
    "js": "js",
    "svelte": "js",
    "sql": "sql",
    "proto": "proto",
    "swift": "swift",
}

_KW = {
    "rust": """as async await break const continue crate dyn else enum extern false fn for
        if impl in let loop match mod move mut pub ref return self Self static struct super
        trait true type unsafe use where while yield""",
    "python": """False None True and as assert async await break class continue def del elif
        else except finally for from global if import in is lambda nonlocal not or pass raise
        return try while with yield match case""",
    "js": """break case catch class const continue debugger default delete do else export
        extends finally for function if import in instanceof new return super switch this
        throw try typeof var void while with yield let static async await of null true false
        undefined interface type enum implements private protected public readonly declare
        namespace abstract as any boolean number string never unknown keyof from get set""",
    "swift": """associatedtype class deinit enum extension fileprivate func import init inout
        internal let open operator private protocol public rethrows static struct subscript
        typealias var break case continue default defer do else fallthrough for guard if in
        repeat return switch where while as catch false is nil self Self super throw throws
        true try async await actor some any""",
    "sql": """select from where insert into values update set delete create table index unique
        primary key not null default integer int text real blob and or order by group having
        limit offset join left right inner outer cross on as in is exists distinct case when
        then else end begin commit rollback transaction pragma without rowid if replace
        collate asc desc union all like glob between cast count sum max min avg coalesce
        ifnull nullif drop alter add column references foreign check conflict ignore abort
        fail returning with recursive view trigger temp temporary virtual using autoincrement
        nocase""",
    "proto": """syntax package import option message enum repeated optional required oneof map
        reserved service rpc returns stream true false double float int32 int64 uint32
        uint64 sint32 sint64 fixed32 fixed64 sfixed32 sfixed64 bool string bytes to max""",
}
KEYWORDS = {lang: frozenset(words.split()) for lang, words in _KW.items()}
KEYWORDS["svelte"] = KEYWORDS["js"]
CASE_INSENSITIVE = {"sql"}

# ---------------------------------------------------------------------------
# Segmentation: comments and string literals; code is what lies between them.

_C_COMMENTS = r"(?P<com>//[^\n]*|/\*[\s\S]*?\*/)"
_SEGMENT = {
    "rust": _C_COMMENTS
    + r"""|(?P<str>(?<!\w)b?r(?P<h>\#*)"[\s\S]*?"(?P=h)|(?<!\w)b?"(?:\\[\s\S]|[^"\\])*"|(?<!\w)b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|[\s\S])|[^'\\\n])')""",
    "python": r"""(?P<com>\#[^\n]*|(?<![\w"'])[rRbBuUfF]{0,2}(?:\"\"\"[\s\S]*?\"\"\"|'''[\s\S]*?'''))"""
    + r"""|(?P<str>(?<![\w"'])[rRbBuUfF]{0,2}(?:"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'))""",
    "js": _C_COMMENTS
    + r"""|(?P<str>"(?:\\[\s\S]|[^"\\\n])*"|'(?:\\[\s\S]|[^'\\\n])*'|`(?:\\[\s\S]|[^`\\])*`)""",
    "svelte": r"(?P<com>//[^\n]*|/\*[\s\S]*?\*/|<!--[\s\S]*?-->)"
    + r"""|(?P<str>"(?:\\[\s\S]|[^"\\\n])*"|'(?:\\[\s\S]|[^'\\\n])*'|`(?:\\[\s\S]|[^`\\])*`)""",
    "swift": _C_COMMENTS
    + r"""|(?P<str>(?P<h>\#*)\"\"\"[\s\S]*?\"\"\"(?P=h)|(?P<h2>\#+)"[\s\S]*?"(?P=h2)|"(?:\\.|[^"\\\n])*")""",
    "css": r"(?P<com>/\*[\s\S]*?\*/)" + r"""|(?P<str>"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')""",
    "scss": _C_COMMENTS + r"""|(?P<str>"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')""",
    "html": r"(?P<com><!--[\s\S]*?-->|/\*[\s\S]*?\*/)" + r"""|(?P<str>"[^"]*"|'[^'\n]*')""",
    "sql": r"(?P<com>--[^\n]*|/\*[\s\S]*?\*/)" + r"""|(?P<str>'(?:''|[^'])*')""",
    "proto": _C_COMMENTS + r"""|(?P<str>"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')""",
}
_SEGMENT_RE = {lang: re.compile(p) for lang, p in _SEGMENT.items()}

_STR_SENTINEL = "\x02"
_CODE_TOKEN = re.compile(
    r"\x02|[A-Za-z_$][A-Za-z0-9_$]*|\d[\w.]*"
    r"|===|!==|\.\.\.|\.\.=|<<=|>>=|::|->|=>|==|!=|<=|>=|&&|\|\||\+=|-=|\*=|/=|%=|\.\.|\?\?|<<|>>|\*\*|[^\s\w]"
)
_IDENT_PART = re.compile(r"[A-Z]+(?![a-z])|[A-Z]?[a-z]+|\d+")
_STRING_WORD = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|\d+")
_COMMENT_WORD = re.compile(r"[a-z0-9]+")
TEXT_WORD = re.compile(r"[a-z0-9]+(?:['’-][a-z0-9]+)*")
_STRING_DELIMS = re.compile(r"""^(?:b?r\#*|[rRbBuUfF]{0,2}|\#+)?("{3}|'{3}|["'`])|("{3}|'{3}|["'`])\#*$""")


def normalize_vocab_word(word: str) -> str:
    """Interop vocabulary and identifiers compare case- and separator-insensitively."""
    return re.sub(r"[_\-.\s]", "", word.lower())


@dataclass
class Lexed:
    language: str
    struct: list = field(default_factory=list)  # normalized struct tokens
    struct_lines: list = field(default_factory=list)
    lex: list = field(default_factory=list)
    lex_lines: list = field(default_factory=list)
    comments: list = field(default_factory=list)  # [(words, lines)]
    texts: list = field(default_factory=list)  # [(words, lines, label)]


class LineIndex:
    def __init__(self, text: str):
        self.starts = [0]
        pos = text.find("\n")
        while pos >= 0:
            self.starts.append(pos + 1)
            pos = text.find("\n", pos + 1)

    def line(self, offset: int) -> int:
        return bisect.bisect_right(self.starts, offset)


def _strip_string_delims(s: str) -> str:
    return _STRING_DELIMS.sub("", s)


def _split_identifier(ident: str):
    return [p.lower() for p in _IDENT_PART.findall(ident)]


def _words_with_lines(word_re, text, first_line):
    """(word, line) for each match of `word_re` in `text`, a comment or string
    literal that starts on `first_line`: a word after a newline inside it is on
    a later line."""
    line, pos = first_line, 0
    for m in word_re.finditer(text):
        line += text.count("\n", pos, m.start())
        pos = m.start()
        yield m.group(0), line


class Lexer:
    """Lexes files for one scan or index build. `mask` holds normalized interop
    vocabulary; `stopwords` are dropped from the lex channel everywhere."""

    def __init__(self, mask=frozenset(), stopwords=frozenset()):
        self.mask = frozenset(mask)
        self.stopwords = frozenset(w.lower() for w in stopwords)

    # -- lex helpers -------------------------------------------------------

    def _lex_identifier(self, ident, line, keywords, out, out_lines, stop=frozenset()):
        if ident in keywords or ident.lower() in stop:
            return
        if self.mask and normalize_vocab_word(ident) in self.mask:
            return
        for part in _split_identifier(ident):
            if part not in stop:
                out.append(part)
                out_lines.append(line)

    def _lex_string(self, content, line, out, out_lines):
        # Stopwords (SQL keywords by default) only apply inside string literals,
        # where embedded SQL would otherwise make keyword runs look like reuse.
        for w, wline in _words_with_lines(_STRING_WORD, content, line):
            if w[0].isdigit():
                out.append(w)
                out_lines.append(wline)
            else:
                self._lex_identifier(w, wline, (), out, out_lines, self.stopwords)

    # -- main entry ----------------------------------------------------------

    def lex(self, text: str, language: str, want_struct=True, want_text_strings=False) -> Lexed:
        if language == "xcstrings":
            return self._lex_xcstrings(text)
        if language == "ftl":
            return self._lex_ftl(text)
        out = Lexed(language)
        lines = LineIndex(text)
        seg_re = _SEGMENT_RE[language]
        keywords = KEYWORDS.get(language, frozenset())
        ci = language in CASE_INSENSITIVE
        family = STRUCT_FAMILY.get(language)
        do_struct = want_struct and family is not None

        code_parts = []  # code text with each string literal replaced by a sentinel
        plain_parts = []  # code text with string literals kept (CSS words)
        strings = []  # (content, line)
        last = 0
        prev_comment_end = None
        for m in seg_re.finditer(text):
            start, end = m.span()
            code_parts.append(text[last:start])
            plain_parts.append(text[last:start])
            if m.group("com") is not None:
                body = m.group("com")
                block_line = lines.line(start)
                words, wlines = [], []
                for w, wline in _words_with_lines(_COMMENT_WORD, body.lower(), block_line):
                    words.append(w)
                    wlines.append(wline)
                gap = text[prev_comment_end:start] if prev_comment_end is not None else None
                if out.comments and gap is not None and gap.strip() == "":
                    out.comments[-1][0].extend(words)
                    out.comments[-1][1].extend(wlines)
                else:
                    out.comments.append((words, wlines))
                prev_comment_end = end
                # Keep line structure for the code tokenizer.
                code_parts.append("\n" * body.count("\n"))
                plain_parts.append("\n" * body.count("\n"))
            else:
                body = m.group(0)
                strings.append((_strip_string_delims(body), lines.line(start)))
                code_parts.append(_STR_SENTINEL + "\n" * body.count("\n"))
                plain_parts.append(body)
            last = end
        code_parts.append(text[last:])
        plain_parts.append(text[last:])
        code = "".join(code_parts)

        struct, struct_lines = out.struct, out.struct_lines
        lex, lex_lines = out.lex, out.lex_lines
        string_iter = iter(strings)
        for lineno, line_text in enumerate(code.split("\n"), start=1):
            if not line_text:
                continue
            for tok in _CODE_TOKEN.findall(line_text):
                c = tok[0]
                if c == _STR_SENTINEL:
                    content, sline = next(string_iter, ("", lineno))
                    if do_struct:
                        struct.append("S")
                        struct_lines.append(lineno)
                    self._lex_string(content, sline, lex, lex_lines)
                elif c.isalpha() or c == "_" or c == "$":
                    word = tok.lower() if ci else tok
                    if do_struct:
                        struct.append(word if word in keywords else "I")
                        struct_lines.append(lineno)
                    self._lex_identifier(word, lineno, keywords, lex, lex_lines)
                elif c.isdigit():
                    if do_struct:
                        struct.append("N")
                        struct_lines.append(lineno)
                    lex.append(tok.lower())
                    lex_lines.append(lineno)
                elif do_struct:
                    struct.append(tok)
                    struct_lines.append(lineno)
        if language == "rust" and do_struct:
            _collapse_rust_attributes(struct, struct_lines)
        if language in ("css", "scss"):
            # CSS words keep their hyphens (`font-size` is one word).
            words, wlines = [], []
            for lineno, line_text in enumerate("".join(plain_parts).split("\n"), start=1):
                for w in TEXT_WORD.findall(line_text.lower()):
                    words.append(w)
                    wlines.append(lineno)
            out.texts.append((words, wlines, "css"))
        if want_text_strings:
            for content, sline in strings:
                pairs = list(_words_with_lines(TEXT_WORD, content.lower(), sline))
                if pairs:
                    out.texts.append(([w for w, _l in pairs], [l for _w, l in pairs], "string"))
        return out

    # -- Fluent and String Catalogs ----------------------------------------

    _FTL_MESSAGE = re.compile(r"^(-?[A-Za-z][\w-]*)\s*=\s?(.*)$")
    _FTL_ATTRIBUTE = re.compile(r"^\s+\.[\w-]+\s*=\s?(.*)$")
    _FTL_PLACEABLE = re.compile(r"\{[^{}\n]*\}")
    _FTL_VARIANT = re.compile(r"^\s*\*?\[[^\]]*\]\s?")
    _FTL_SYNTAX = re.compile(r"[{}]|\$[\w-]+|(?<!\w)-[A-Za-z][\w-]*|->")

    def _lex_ftl(self, text: str) -> Lexed:
        out = Lexed("ftl")
        words, wlines, label = [], [], None

        def flush():
            if words:
                out.texts.append((list(words), list(wlines), label))
            words.clear()
            wlines.clear()

        def add(fragment, lineno):
            fragment = self._FTL_PLACEABLE.sub(" ", fragment)
            fragment = self._FTL_SYNTAX.sub(" ", self._FTL_VARIANT.sub("", fragment))
            for w in TEXT_WORD.findall(fragment.lower()):
                words.append(w)
                wlines.append(lineno)

        for lineno, line in enumerate(text.split("\n"), start=1):
            if line.startswith("#") or not line.strip():
                if line.startswith("#"):
                    flush()
                continue
            m = self._FTL_MESSAGE.match(line)
            if m:
                flush()
                label = m.group(1)
                add(m.group(2), lineno)
                continue
            m = self._FTL_ATTRIBUTE.match(line)
            if m:
                flush()
                add(m.group(1), lineno)
                continue
            if line[:1].isspace():
                add(line, lineno)
        flush()
        return out

    _FORMAT_SPECIFIER = re.compile(r"%(?:\d+\$)?[-+ #0]*\d*(?:\.\d+)?(?:hh|h|ll|l|q|z|t|j)?[@dDuUxXoOfeEgGcCsSpaA%]")

    def _lex_xcstrings(self, text: str) -> Lexed:
        out = Lexed("xcstrings")
        catalog = json.loads(text)
        lines = LineIndex(text)
        source = catalog.get("sourceLanguage", "en")
        for key, entry in (catalog.get("strings") or {}).items():
            values = []
            loc = (entry or {}).get("localizations", {}).get("en")
            if loc:
                _collect_string_units(loc, values)
            elif source == "en":
                values.append(key)
            pos = text.find(json.dumps(key, ensure_ascii=False))
            line = lines.line(pos) if pos >= 0 else 1
            for value in values:
                value = self._FORMAT_SPECIFIER.sub(" ", value)
                words = TEXT_WORD.findall(value.lower())
                if words:
                    out.texts.append((words, [line] * len(words), key))
        return out


def _collapse_rust_attributes(tokens, lines):
    """Replaces each `#[...]` / `#![...]` attribute by one `#[]` token, in place.
    Derive lists and other attributes are boilerplate that would otherwise make
    unrelated type declarations look alike."""
    out_t, out_l, i, n = [], [], 0, len(tokens)
    while i < n:
        j = i + 1
        if tokens[i] == "#" and j < n and tokens[j] == "!":
            j += 1
        if tokens[i] == "#" and j < n and tokens[j] == "[":
            depth, j = 0, j
            while j < n:
                if tokens[j] == "[":
                    depth += 1
                elif tokens[j] == "]":
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            out_t.append("#[]")
            out_l.append(lines[i])
            i = j + 1
            continue
        out_t.append(tokens[i])
        out_l.append(lines[i])
        i += 1
    tokens[:] = out_t
    lines[:] = out_l


def _collect_string_units(node, values):
    if isinstance(node, dict):
        unit = node.get("stringUnit")
        if isinstance(unit, dict) and isinstance(unit.get("value"), str):
            values.append(unit["value"])
        for k, v in node.items():
            if k != "stringUnit":
                _collect_string_units(v, values)
    elif isinstance(node, list):
        for v in node:
            _collect_string_units(v, values)


def language_of(path: str):
    dot = path.rfind(".")
    if dot < 0:
        return None
    return EXTENSION_LANGUAGE.get(path[dot:].lower())
