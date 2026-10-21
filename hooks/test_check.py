"""Tests of the checks run before each commit and push: python -m unittest discover -s hooks"""
import json
import pathlib
import tempfile
import unittest

import check

# Built in pieces, so that this file does not trip the checks it tests.
GOOGLE_KEY = "AI" + "za" + "x" * 35
PRIVATE_KEY = "-----BEGIN " + "PRIVATE KEY-----"
SERVICE_ACCOUNT_KEY = '  "private' + '_key": "x",'


class AddedLines(unittest.TestCase):
    def test_reads_the_lines_a_diff_adds_with_their_file_and_number(self):
        diff = "\n".join([
            "diff --git a/src/a.rs b/src/a.rs",
            "--- a/src/a.rs",
            "+++ b/src/a.rs",
            "@@ -3,0 +4,2 @@ fn main() {",
            "+let x = 1;",
            "+++ not a header inside a hunk",
            "@@ -10 +12 @@",
            "-old",
            "+new",
            "diff --git a/gone.txt b/gone.txt",
            "--- a/gone.txt",
            "+++ /dev/null",
            "@@ -1 +0,0 @@",
            "-bye",
        ])
        self.assertEqual(
            check.added_lines(diff),
            [("src/a.rs", 4, "let x = 1;"), ("src/a.rs", 5, "++ not a header inside a hunk"), ("src/a.rs", 12, "new")],
        )


class Problems(unittest.TestCase):
    secrets = {"s3cr3t-value-of-prim": "PRIM_API_KEY in .env"}
    words = ["Gare du Moulin", "nobody@example.org"]

    def test_a_known_secret_is_named_by_where_it_comes_from_not_shown(self):
        found = check.problems([("README.md", 7, "key = s3cr3t-value-of-prim")], self.secrets, [])
        self.assertEqual(found, ["README.md:7: the value of PRIM_API_KEY in .env"])

    def test_keys_of_known_formats_are_refused(self):
        lines = [
            ("a.json", 1, SERVICE_ACCOUNT_KEY),
            ("b.pem", 2, PRIVATE_KEY),
            ("c.kt", 3, f'val key = "{GOOGLE_KEY}"'),
        ]
        found = check.problems(lines, {}, [])
        self.assertEqual([problem.split(":")[0] for problem in found], ["a.json", "b.pem", "c.kt"])
        self.assertTrue(all("looks like" in problem for problem in found), found)

    def test_personal_words_are_refused_whatever_their_case(self):
        found = check.problems([("x.md", 1, "From gare du moulin at 8")], {}, self.words)
        self.assertEqual(found, ["x.md:1: a word of local/forbidden.txt (Gare du Moulin)"])

    def test_ordinary_lines_pass(self):
        self.assertEqual(check.problems([("x.rs", 1, "let gare = Centrale;")], self.secrets, self.words), [])

    def test_files_that_hold_secrets_are_refused_by_their_path(self):
        paths = [".env", "android/local.properties", "android/app/google-services.json", "local/config.toml",
                 "captures/x.json", "release.jks", "backend/src/main.rs"]
        self.assertEqual(check.path_problems(paths), [f"{path}: a file that stays out of the repository" for path in paths[:-1]])

    def test_an_author_address_may_not_be_a_personal_one(self):
        self.assertEqual(
            check.email_problems(["12766525+nonock@users.noreply.github.com", "Nobody@Example.org"], self.words),
            ["commit by Nobody@Example.org: a word of local/forbidden.txt (nobody@example.org)"],
        )


class Loading(unittest.TestCase):
    def test_secrets_and_words_come_from_the_local_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "android/app").mkdir(parents=True)
            (root / "local").mkdir()
            (root / ".env").write_text("# comment\nPRIM_API_KEY=prim-key-0123456789\nSHORT=abc\n", encoding="utf-8")
            (root / "android/local.properties").write_text(
                "sdk.dir=C\\:\\\\Android\neztravel.appKey=app-key-0123456789abcdef\n", encoding="utf-8")
            (root / "local/firebase-service-account.json").write_text(json.dumps({
                "type": "service_account", "private_key_id": "0123456789abcdef", "client_id": "123456789012345",
                "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            }), encoding="utf-8")
            (root / "android/app/google-services.json").write_text(json.dumps({
                "client": [{"api_key": [{"current_key": "google-key-0123456789"}]}],
            }), encoding="utf-8")
            (root / "local/forbidden.txt").write_text("# my stations\nGare du Moulin\n\n", encoding="utf-8")
            (root / "local/config.toml").write_text(
                'board = "stop_area:IDFM:424242"\nline = "C09999"\n[[lines]]\ncode = "C08888"\n', encoding="utf-8")
            self.assertEqual(
                check.load_secrets(root),
                {
                    "prim-key-0123456789": "PRIM_API_KEY in .env",
                    "app-key-0123456789abcdef": "eztravel.appKey in android/local.properties",
                    "0123456789abcdef": "private_key_id in local/firebase-service-account.json",
                    "123456789012345": "client_id in local/firebase-service-account.json",
                    "google-key-0123456789": "current_key in android/app/google-services.json",
                },
            )
            self.assertEqual(
                check.load_words(root),
                ["Gare du Moulin", "stop_area:IDFM:424242", "C08888", "C09999"],
            )

    def test_a_word_marked_with_an_exclamation_mark_is_allowed(self):
        # Such as a stop of the commute that the repository's example already uses for another line.
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "local").mkdir()
            (root / "local/forbidden.txt").write_text("Gare du Moulin\n!stop_area:IDFM:424242\n", encoding="utf-8")
            (root / "local/config.toml").write_text(
                'board = "stop_area:IDFM:424242"\nalight = "stop_area:IDFM:515151"\n', encoding="utf-8")
            self.assertEqual(check.load_words(root), ["Gare du Moulin", "stop_area:IDFM:515151"])

    def test_without_local_files_only_the_known_formats_are_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            self.assertEqual((check.load_secrets(root), check.load_words(root)), ({}, []))


if __name__ == "__main__":
    unittest.main()
