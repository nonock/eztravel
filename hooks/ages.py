"""Refuses dependency versions published less than two weeks ago, but a fix of a known vulnerability.

    python hooks/ages.py <base> [<head>]

Looks at the crates of backend/Cargo.lock, the versioned artifacts and plugins
of android/gradle/libs.versions.toml and the Gradle version of its wrapper that
changed from <base> to <head> (HEAD by default). A version newly published may be a compromised one, pulled
days later: it waits two weeks. It passes at once when the version it replaces
has a vulnerability it no longer has (OSV.dev), or when hooks/fresh-allowed.txt
lists it with its advisory: "<name> <version> <advisory> [why]".
"""
import datetime
import email.utils
import json
import pathlib
import re
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request

MIN_AGE_DAYS = 14
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
MAVEN_REPOSITORIES = [
    "https://dl.google.com/android/maven2",
    "https://repo1.maven.org/maven2",
    "https://plugins.gradle.org/m2",
]
USER_AGENT = "eztravel-age-check (https://github.com/nonock/eztravel)"
ADVISORY = re.compile(r"^(RUSTSEC-\d{4}-\d{4}|CVE-\d{4}-\d+|GHSA(-[0-9a-z]{4}){3})$")


def crates(lock_text):
    """(name, version) of each crate from crates.io in a Cargo.lock."""
    packages = tomllib.loads(lock_text).get("package", [])
    return {(package["name"], package["version"]) for package in packages if package.get("source") == CRATES_IO}


def artifacts(catalog_text):
    """("group:name", version) of each versioned library and plugin of a Gradle catalog."""
    catalog = tomllib.loads(catalog_text)
    versions = catalog.get("versions", {})

    def version_of(entry):
        version = entry.get("version")
        return versions.get(version["ref"]) if isinstance(version, dict) else version

    found = set()
    for library in catalog.get("libraries", {}).values():
        coordinate = library.get("module") or f"{library.get('group')}:{library.get('name')}"
        if version_of(library):
            found.add((coordinate, version_of(library)))
    for plugin in catalog.get("plugins", {}).values():
        if version_of(plugin):
            found.add((f"{plugin['id']}:{plugin['id']}.gradle.plugin", version_of(plugin)))
    return found


def wrapper(properties_text):
    """("gradle", version) of the Gradle wrapper, from its distribution's address."""
    found = re.search(r"distributions/gradle-([0-9][^-/]*)-(?:bin|all)\.zip", properties_text)
    return {("gradle", found.group(1))} if found else set()


def gradle_build_time(versions, version):
    """When `version` was built, from services.gradle.org's list of versions."""
    for release in versions:
        if release.get("version") == version:
            return datetime.datetime.strptime(release["buildTime"], "%Y%m%d%H%M%S%z")
    return None


def allowed(text):
    """The versions hooks/fresh-allowed.txt lets through, with their advisory."""
    found = {}
    for line in text.splitlines():
        fields = line.split()
        if len(fields) >= 3 and not fields[0].startswith("#") and ADVISORY.match(fields[2]):
            found[(fields[0], fields[1])] = fields[2]
    return found


def version_key(version):
    return [int(part) if part.isdigit() else part for part in re.split(r"[.\-+]", version)]


def changes(base, head):
    """(name, version replaced or None, new version) of each version new in head."""
    gone = base - head
    found = []
    for name, version in sorted(head - base):
        replaced = sorted((old for other, old in gone if other == name), key=version_key)
        found.append((name, replaced[-1] if replaced else None, version))
    return found


def verdict(change, now, published, vulnerabilities, allowed_versions):
    """(passes, why) for one change; `published` and `vulnerabilities` look the version up."""
    name, old, new = change
    advisory = allowed_versions.get((name, new))
    when = published(name, new)
    if when is None:
        if advisory:
            return True, f"{name} {new}: publication date not found, allowed for {advisory}"
        return False, f"{name} {new}: publication date not found"
    days = (now - when).days
    age = f"{name} {new}: published {days} {'day' if days == 1 else 'days'} ago"
    if days >= MIN_AGE_DAYS:
        return True, age
    if advisory:
        return True, f"{age}, allowed for {advisory}"
    if old is not None:
        fixed = vulnerabilities(name, old) - vulnerabilities(name, new)
        if fixed:
            return True, f"{age}, fixes {', '.join(sorted(fixed))}"
    return False, f"{age}, less than {MIN_AGE_DAYS}"


def crate_published(name, version):
    request = urllib.request.Request(f"https://crates.io/api/v1/crates/{name}/{version}")
    request.add_header("User-Agent", USER_AGENT)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            created = json.load(response)["version"]["created_at"]
    except (urllib.error.URLError, KeyError, ValueError):
        return None
    return datetime.datetime.fromisoformat(created.replace("Z", "+00:00"))


def artifact_published(coordinate, version):
    group, name = coordinate.split(":")
    path = f"{group.replace('.', '/')}/{name}/{version}/{name}-{version}.pom"
    for repository in MAVEN_REPOSITORIES:
        request = urllib.request.Request(f"{repository}/{path}", method="HEAD")
        request.add_header("User-Agent", USER_AGENT)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                modified = response.headers.get("Last-Modified")
        except urllib.error.URLError:
            continue
        if modified:
            return email.utils.parsedate_to_datetime(modified)
    return None


def gradle_published(name, version):
    request = urllib.request.Request("https://services.gradle.org/versions/all")
    request.add_header("User-Agent", USER_AGENT)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return gradle_build_time(json.load(response), version)
    except (urllib.error.URLError, ValueError):
        return None


def none_known(name, version):
    """Gradle's own advisories are not looked up: a fresh Gradle needs a line of fresh-allowed.txt."""
    return set()


def vulnerabilities_in(ecosystem):
    def look_up(name, version):
        query = json.dumps({"version": version, "package": {"name": name, "ecosystem": ecosystem}}).encode()
        request = urllib.request.Request("https://api.osv.dev/v1/query", data=query, method="POST")
        request.add_header("User-Agent", USER_AGENT)
        request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return {vulnerability["id"] for vulnerability in json.load(response).get("vulns", [])}
        except (urllib.error.URLError, ValueError):
            return set()

    return look_up


def git_file(revision, path):
    result = subprocess.run(["git", "show", f"{revision}:{path}"], capture_output=True)
    return result.stdout.decode("utf-8") if result.returncode == 0 else ""


def main(arguments):
    if len(arguments) not in (1, 2):
        print(__doc__, file=sys.stderr)
        return 2
    base, head = arguments[0], arguments[1] if len(arguments) == 2 else "HEAD"
    root = pathlib.Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, check=True).stdout.decode().strip())
    listed = root / "hooks/fresh-allowed.txt"
    allowed_versions = allowed(listed.read_text(encoding="utf-8")) if listed.exists() else {}
    now = datetime.datetime.now(datetime.timezone.utc)
    checks = [
        ("backend/Cargo.lock", crates, crate_published, vulnerabilities_in("crates.io")),
        ("android/gradle/libs.versions.toml", artifacts, artifact_published, vulnerabilities_in("Maven")),
        ("android/gradle/wrapper/gradle-wrapper.properties", wrapper, gradle_published, none_known),
    ]
    refused = 0
    for path, read, published, vulnerabilities in checks:
        for change in changes(read(git_file(base, path)), read(git_file(head, path))):
            passes, why = verdict(change, now, published, vulnerabilities, allowed_versions)
            print(f"ages: {'ok' if passes else 'refused'}: {why}", file=sys.stdout if passes else sys.stderr)
            refused += not passes
    return 1 if refused else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
