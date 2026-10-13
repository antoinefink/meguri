"""Lexing: what each channel sees for each language."""

import json
import unittest

from meguri_similarity.lexer import Lexer, language_of, normalize_vocab_word


class RustLexing(unittest.TestCase):
    def setUp(self):
        self.lx = Lexer()

    def test_identifiers_abstract_and_keywords_stay_in_struct(self):
        out = self.lx.lex("pub fn total(items: &[u32]) -> u32 { items.iter().sum() }", "rust")
        self.assertEqual(out.struct[:6], ["pub", "fn", "I", "(", "I", ":"])
        self.assertNotIn("total", out.struct)
        self.assertIn("->", out.struct)

    def test_attributes_collapse_to_one_struct_token(self):
        src = "#[derive(Debug, Clone)]\n#![allow(dead_code)]\nstruct A { x: u8 }"
        out = self.lx.lex(src, "rust")
        self.assertEqual(out.struct[:4], ["#[]", "#[]", "struct", "I"])
        self.assertEqual(out.struct_lines[:3], [1, 2, 3])
        self.assertIn("debug", out.lex)

    def test_lifetimes_are_not_char_literals(self):
        src = "fn f<'a>(x: &'a str, c: char) -> bool { c == 'z' && x.len() > 0 }"
        out = self.lx.lex(src, "rust")
        self.assertEqual(out.struct.count("S"), 1, out.struct)
        self.assertIn("z", out.lex)

    def test_raw_strings_and_comments(self):
        src = 'let q = r#"select "x" from t"#; // trailing words here\n/* block\ncomment */ let y = 2;'
        out = self.lx.lex(src, "rust")
        self.assertEqual(out.struct.count("S"), 1)
        self.assertEqual(len(out.comments), 1)
        words, lines = out.comments[0]
        self.assertEqual(words, ["trailing", "words", "here", "block", "comment"])
        self.assertEqual(lines, [1, 1, 1, 2, 3])
        self.assertEqual(out.struct_lines[-1], 3)

    def test_separated_comments_are_separate_blocks(self):
        src = "// first block\nlet a = 1;\n// second block\n"
        out = self.lx.lex(src, "rust")
        self.assertEqual([w for w, _ in out.comments], [["first", "block"], ["second", "block"]])

    def test_lex_splits_identifiers_and_keeps_literals(self):
        out = self.lx.lex('let dueDate = next_review_at(3, "Hello world");', "rust")
        self.assertEqual(out.lex, ["due", "date", "next", "review", "at", "3", "hello", "world"])

    def test_mask_drops_interop_identifiers_in_code_and_strings(self):
        lx = Lexer(mask={normalize_vocab_word("odue"), normalize_vocab_word("last_modified")})
        out = lx.lex('let lastModified = row.odue + 1; let s = "set odue = 0";', "rust")
        self.assertNotIn("odue", out.lex)
        self.assertNotIn("modified", out.lex)
        self.assertIn("row", out.lex)

    def test_words_in_multiline_strings_carry_their_own_line(self):
        src = 'let q = r#"alpha\nbeta gamma\n\ndelta"#;\nlet s = "one\ntwo";\nlet z = 7;'
        out = self.lx.lex(src, "rust", want_text_strings=True)
        self.assertEqual(
            list(zip(out.lex, out.lex_lines)),
            [("q", 1), ("alpha", 1), ("beta", 2), ("gamma", 2), ("delta", 4), ("s", 5), ("one", 5), ("two", 6),
             ("z", 7), ("7", 7)],
        )
        self.assertEqual(
            [(w, l) for words, lines, _k in out.texts for w, l in zip(words, lines)],
            [("alpha", 1), ("beta", 2), ("gamma", 2), ("delta", 4), ("one", 5), ("two", 6)],
        )

    def test_stopwords_apply_inside_strings_only(self):
        lx = Lexer(stopwords={"select", "from", "where"})
        out = lx.lex('fn select(from: u8) { q("select id from cards where x") }', "rust")
        self.assertIn("select", out.lex)  # an identifier in code
        self.assertEqual(out.lex[-3:], ["id", "cards", "x"])


class OtherLanguages(unittest.TestCase):
    def setUp(self):
        self.lx = Lexer()

    def test_python_docstrings_are_comments(self):
        src = 'def f(x):\n    """Return the double of x."""\n    return x * 2  # twice\n'
        out = self.lx.lex(src, "python")
        self.assertEqual([w for w, _ in out.comments], [["return", "the", "double", "of", "x"], ["twice"]])
        self.assertEqual(out.struct[:2], ["def", "I"])

    def test_swift_and_js_multiline_strings_carry_each_words_line(self):
        swift = 'let s = """\n    first words\n    second\n    """\nlet t = 1\n'
        out = self.lx.lex(swift, "swift")
        self.assertEqual(list(zip(out.lex, out.lex_lines)), [("s", 1), ("first", 2), ("words", 2), ("second", 3), ("t", 5), ("1", 5)])
        js = "const t = `one\ntwo ${x}\nthree`;\n"
        out = self.lx.lex(js, "js")
        self.assertEqual(list(zip(out.lex, out.lex_lines)), [("t", 1), ("one", 1), ("two", 2), ("x", 2), ("three", 3)])

    def test_sql_keywords_are_case_insensitive(self):
        out = self.lx.lex("SELECT id FROM cards WHERE due <= ? -- due cards\n", "sql")
        self.assertEqual(out.struct[:4], ["select", "I", "from", "I"])
        self.assertEqual(out.lex, ["id", "cards", "due"])
        self.assertEqual(out.comments[0][0], ["due", "cards"])

    def test_css_words_keep_hyphens(self):
        out = self.lx.lex(".card { font-family: sans-serif; /* note */ font-size: 20px; }", "css")
        words = out.texts[0][0]
        self.assertEqual(words, ["card", "font-family", "sans-serif", "font-size", "20px"])
        self.assertEqual(out.comments[0][0], ["note"])

    def test_fluent_values_without_syntax(self):
        src = (
            "# A comment\n"
            "deck-count = { $count ->\n"
            "    [one] { $count } deck selected\n"
            "   *[other] { $count } decks selected\n"
            "  }\n"
            "about-title = About { -brand }\n"
            "    .tooltip = Shows the version\n"
        )
        out = self.lx.lex(src, "ftl")
        seqs = [w for w, _l, _k in out.texts]
        self.assertEqual(seqs, [["deck", "selected", "decks", "selected"], ["about"], ["shows", "the", "version"]])

    def test_string_catalog_english_values(self):
        catalog = {
            "sourceLanguage": "en",
            "strings": {
                "Again": {},
                "cards.count": {
                    "localizations": {
                        "en": {
                            "variations": {
                                "plural": {
                                    "one": {"stringUnit": {"state": "translated", "value": "%lld card"}},
                                    "other": {"stringUnit": {"state": "translated", "value": "%lld cards"}},
                                }
                            }
                        },
                        "fr": {"stringUnit": {"state": "translated", "value": "%lld cartes"}},
                    }
                },
            },
            "version": "1.0",
        }
        out = self.lx.lex(json.dumps(catalog, indent=2), "xcstrings")
        got = sorted((k, tuple(w)) for w, _l, k in out.texts)
        self.assertEqual(got, [("Again", ("again",)), ("cards.count", ("card",)), ("cards.count", ("cards",))])

    def test_languages_by_extension(self):
        self.assertEqual(language_of("app/Foo.swift"), "swift")
        self.assertEqual(language_of("x/Localizable.xcstrings"), "xcstrings")
        self.assertEqual(language_of("ts/a.svelte"), "svelte")
        self.assertIsNone(language_of("README"))
        self.assertIsNone(language_of("run.sh"))


if __name__ == "__main__":
    unittest.main()
