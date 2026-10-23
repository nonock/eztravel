"""Tests of the dependency age check: python -m unittest discover -s hooks"""
import datetime
import unittest

import ages

NOW = datetime.datetime(2026, 10, 21, 12, 0, tzinfo=datetime.timezone.utc)


def days_ago(days):
    return NOW - datetime.timedelta(days=days)


LOCK = """
version = 4

[[package]]
name = "eztravel-backend"
version = "0.1.0"

[[package]]
name = "rustls-webpki"
version = "{webpki}"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"

[[package]]
name = "windows-sys"
version = "0.52.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "windows-sys"
version = "0.59.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
{extra}"""

CATALOG = """
[versions]
agp = "{agp}"
composeBom = "2026.09.00"
junit = "4.13.2"

[libraries]
junit = {{ group = "junit", name = "junit", version.ref = "junit" }}
androidx-compose-bom = {{ group = "androidx.compose", name = "compose-bom", version.ref = "composeBom" }}
androidx-ui = {{ group = "androidx.compose.ui", name = "ui" }}
json = {{ module = "org.json:json", version = "20250517" }}

[plugins]
android-application = {{ id = "com.android.application", version.ref = "agp" }}
"""


class Reading(unittest.TestCase):
    def test_registry_crates_are_read_from_the_lock_file(self):
        self.assertEqual(
            ages.crates(LOCK.format(webpki="0.103.15", extra="")),
            {("rustls-webpki", "0.103.15"), ("windows-sys", "0.52.0"), ("windows-sys", "0.59.0")},
        )

    def test_versioned_artifacts_and_plugins_are_read_from_the_catalog(self):
        self.assertEqual(
            ages.artifacts(CATALOG.format(agp="9.3.0")),
            {
                ("junit:junit", "4.13.2"),
                ("androidx.compose:compose-bom", "2026.09.00"),
                ("org.json:json", "20250517"),
                ("com.android.application:com.android.application.gradle.plugin", "9.3.0"),
            },
        )

    def test_the_gradle_version_is_read_from_the_wrapper(self):
        text = "distributionBase=GRADLE_USER_HOME\ndistributionUrl=https\\://services.gradle.org/distributions/gradle-9.8.1-bin.zip\n"
        self.assertEqual(ages.wrapper(text), {("gradle", "9.8.1")})
        self.assertEqual(ages.wrapper(""), set())

    def test_a_gradle_release_is_dated_by_its_build(self):
        versions = [
            {"version": "9.8.1", "buildTime": "20261007165746+0000"},
            {"version": "9.8.0", "buildTime": "20260924134000+0000"},
        ]
        self.assertEqual(
            ages.gradle_build_time(versions, "9.8.1"),
            datetime.datetime(2026, 10, 7, 16, 57, 46, tzinfo=datetime.timezone.utc),
        )
        self.assertIsNone(ages.gradle_build_time(versions, "9.9.0"))

    def test_allowed_versions_need_an_advisory(self):
        text = "# comment\nrustls-webpki 0.103.16 RUSTSEC-2026-0382 certificates\nlonely 1.0.0\n\n"
        self.assertEqual(ages.allowed(text), {("rustls-webpki", "0.103.16"): "RUSTSEC-2026-0382"})


class Changes(unittest.TestCase):
    def test_a_new_version_is_paired_with_the_one_it_replaces(self):
        base = {("rustls-webpki", "0.103.15"), ("windows-sys", "0.52.0")}
        head = {("rustls-webpki", "0.103.16"), ("windows-sys", "0.52.0"), ("jiff", "0.2.15")}
        self.assertEqual(
            ages.changes(base, head),
            [("jiff", None, "0.2.15"), ("rustls-webpki", "0.103.15", "0.103.16")],
        )


class Verdicts(unittest.TestCase):
    def verdict(self, change, published, vulnerable=(), allowed=None):
        def vulnerabilities(name, version):
            return {"RUSTSEC-2026-0382"} if (name, version) in vulnerable else set()

        return ages.verdict(change, NOW, lambda name, version: published, vulnerabilities, allowed or {})

    def test_a_version_two_weeks_old_passes(self):
        self.assertEqual(self.verdict(("jiff", None, "0.2.15"), days_ago(14)), (True, "jiff 0.2.15: published 14 days ago"))

    def test_a_newer_version_is_refused(self):
        self.assertEqual(
            self.verdict(("jiff", "0.2.14", "0.2.15"), days_ago(3)),
            (False, "jiff 0.2.15: published 3 days ago, less than 14"),
        )

    def test_a_newer_version_passes_when_it_fixes_a_known_vulnerability(self):
        ok, message = self.verdict(("rustls-webpki", "0.103.15", "0.103.16"), days_ago(1), vulnerable={("rustls-webpki", "0.103.15")})
        self.assertEqual((ok, message), (True, "rustls-webpki 0.103.16: published 1 day ago, fixes RUSTSEC-2026-0382"))

    def test_a_fix_that_is_still_vulnerable_is_no_fix(self):
        vulnerable = {("rustls-webpki", "0.103.15"), ("rustls-webpki", "0.103.16")}
        ok, _ = self.verdict(("rustls-webpki", "0.103.15", "0.103.16"), days_ago(1), vulnerable=vulnerable)
        self.assertFalse(ok)

    def test_a_newer_version_passes_when_allowed_with_its_advisory(self):
        allowed = {("jiff", "0.2.15"): "GHSA-xxxx-yyyy-zzzz"}
        self.assertEqual(
            self.verdict(("jiff", None, "0.2.15"), days_ago(0), allowed=allowed),
            (True, "jiff 0.2.15: published 0 days ago, allowed for GHSA-xxxx-yyyy-zzzz"),
        )

    def test_a_version_without_a_known_date_is_refused_unless_allowed(self):
        self.assertEqual(self.verdict(("x", None, "1.0.0"), None), (False, "x 1.0.0: publication date not found"))
        self.assertTrue(self.verdict(("x", None, "1.0.0"), None, allowed={("x", "1.0.0"): "CVE-2026-1"})[0])


if __name__ == "__main__":
    unittest.main()
