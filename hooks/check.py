"""Refuses secrets and personal data on their way into the repository, which is public.

    python hooks/check.py staged         the changes about to be committed
    python hooks/check.py commits <sha>  every commit up to <sha> not yet on a remote

Secrets are the actual values of the local files that hold them, plus keys of
known formats. Personal data is the words listed in local/forbidden.txt, one per
line, with the stops and lines of local/config.toml: both stay out of the
repository, so a fresh clone checks the known formats only.
"""
import json
import pathlib
import re
import subprocess
import sys

# Shorter values are too common to tell apart from ordinary text.
MIN_SECRET = 12

KNOWN_FORMATS = [
    (re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"), "a private key"),
    (re.compile(r'"private_key"\s*:'), "a service account key"),
    (re.compile(r"AIza[0-9A-Za-z_\-]{35}"), "a Google API key"),
    (re.compile(r"ya29\.[0-9A-Za-z_\-]{20,}"), "a Google access token"),
    (re.compile(r"APA91b[0-9A-Za-z_\-]{50,}"), "a Firebase device token"),
]

OUT_OF_REPOSITORY = {".env", "local.properties", "google-services.json"}


def added_lines(diff):
    """(path, line number, text) of each line a unified diff adds."""
    lines, path, number, in_hunk = [], None, 0, False
    for line in diff.splitlines():
        if line.startswith("diff --git "):
            in_hunk = False
        elif not in_hunk and line.startswith("+++ "):
            path = line[6:] if line.startswith("+++ b/") else None
        elif line.startswith("@@"):
            in_hunk = True
            number = int(re.match(r"@@ -\S+ \+(\d+)", line).group(1))
        elif in_hunk and line.startswith("+"):
            if path is not None:
                lines.append((path, number, line[1:]))
            number += 1
    return lines


def problems(added, secrets, words):
    found = []
    for path, number, text in added:
        where = f"{path}:{number}"
        found += [f"{where}: the value of {label}" for value, label in secrets.items() if value in text]
        found += [f"{where}: looks like {name}" for pattern, name in KNOWN_FORMATS if pattern.search(text)]
        found += [f"{where}: a word of local/forbidden.txt ({word})" for word in words if word.lower() in text.lower()]
    return found


def path_problems(paths):
    def kept_out(path):
        name = path.rsplit("/", 1)[-1]
        return (
            name in OUT_OF_REPOSITORY
            or name.endswith((".jks", ".keystore"))
            or path.startswith(("local/", "captures/"))
        )

    return [f"{path}: a file that stays out of the repository" for path in paths if kept_out(path)]


def email_problems(emails, words):
    return [
        f"commit by {email}: a word of local/forbidden.txt ({word})"
        for email in dict.fromkeys(emails)
        for word in words
        if word.lower() in email.lower()
    ]


def load_secrets(root):
    secrets = {}

    def keep(value, label):
        if isinstance(value, str) and len(value) >= MIN_SECRET:
            secrets[value] = label

    env = root / ".env"
    if env.exists():
        for line in env.read_text(encoding="utf-8").splitlines():
            if "=" in line and not line.lstrip().startswith("#"):
                name, value = line.split("=", 1)
                keep(value.strip().strip("\"'"), f"{name.strip()} in .env")
    properties = root / "android/local.properties"
    if properties.exists():
        for line in properties.read_text(encoding="utf-8").splitlines():
            if "=" in line and not line.lstrip().startswith("#"):
                name, value = (part.strip() for part in line.split("=", 1))
                if "key" in name.lower():
                    keep(value, f"{name} in android/local.properties")
    account = root / "local/firebase-service-account.json"
    if account.exists():
        fields = json.loads(account.read_text(encoding="utf-8"))
        for name in ("private_key_id", "client_email", "client_id", "project_id"):
            keep(fields.get(name), f"{name} in local/firebase-service-account.json")
        for line in fields.get("private_key", "").splitlines():
            if not line.startswith("-----"):
                keep(line, "private_key in local/firebase-service-account.json")
    services = root / "android/app/google-services.json"
    if services.exists():
        def walk(value):
            if isinstance(value, dict):
                for name, inner in value.items():
                    if name in ("current_key", "mobilesdk_app_id", "project_id", "project_number"):
                        keep(inner, f"{name} in android/app/google-services.json")
                    walk(inner)
            elif isinstance(value, list):
                for inner in value:
                    walk(inner)

        walk(json.loads(services.read_text(encoding="utf-8")))
    return secrets


def load_words(root):
    words = []
    listed = root / "local/forbidden.txt"
    if listed.exists():
        for line in listed.read_text(encoding="utf-8").splitlines():
            if line.strip() and not line.lstrip().startswith("#"):
                words.append(line.strip())
    config = root / "local/config.toml"
    if config.exists():
        text = config.read_text(encoding="utf-8")
        words += sorted(set(re.findall(r"stop_area:IDFM:\d+", text)))
        words += sorted(set(re.findall(r'(?:line|code) = "(C\d+)"', text)))
    return words


def git(*arguments):
    result = subprocess.run(["git", *arguments], capture_output=True, check=True)
    return result.stdout.decode("utf-8", errors="replace")


def main(arguments):
    root = pathlib.Path(git("rev-parse", "--show-toplevel").strip())
    secrets, words = load_secrets(root), load_words(root)
    found = []
    if arguments == ["staged"]:
        diff = git("diff", "--cached", "-U0", "--no-color", "--no-ext-diff")
        paths = git("diff", "--cached", "--name-only", "--diff-filter=ACMR").splitlines()
        found = path_problems(paths) + problems(added_lines(diff), secrets, words)
    elif len(arguments) == 2 and arguments[0] == "commits":
        commits = git("rev-list", arguments[1], "--not", "--remotes").split()
        for commit in commits:
            show = ("show", "--format=", "--diff-merges=first-parent", "--no-color", "--no-ext-diff")
            diff = git(*show, "-U0", commit)
            paths = git(*show, "--name-only", "--diff-filter=ACMR", commit).splitlines()
            emails = git("show", "-s", "--format=%ae%n%ce", commit).split()
            found += [
                f"{commit[:7]} {problem}"
                for problem in path_problems(paths) + problems(added_lines(diff), secrets, words) + email_problems(emails, words)
            ]
        print(f"check: {len(commits)} commit(s) to push looked at")
    else:
        print(__doc__, file=sys.stderr)
        return 2
    for problem in found:
        print(f"refused: {problem}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
