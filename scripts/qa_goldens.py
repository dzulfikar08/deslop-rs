#!/usr/bin/env python3
"""Differential QA: run the Rust binary over the Haskell suite's fixture
goldens and diff the transcripts.

Usage: qa_goldens.py [name-substring ...]   (no args = all)
"""
import difflib
import os
import re
import shutil
import subprocess
import sys
import tempfile

HS = os.environ.get("DESLOP_HS", "/Users/macbookpro/Documents/JoyoDigitama/deslop")
BIN = os.environ.get(
    "DESLOP_BIN", "/Users/macbookpro/Documents/JoyoDigitama/deslop-rs/target/debug/deslop"
)
FIXTURES = os.path.join(HS, "fixtures")
GOLDEN_DIR = os.path.join(HS, ".golden")

PROJECTS = [
    "ts-project-1",
    "ixartz-next-js-boilerplate",
    "melzar-nextjs-clean-architecture",
    "ts-cycles-project",
    "ts-gitignore-project",
    "ts-globplus-project",
    "ts-casing-project",
]

# The files each fix golden snapshots, in spec order (ProjectGoldenSpec.hs).
FIX_SNAPSHOTS = {
    "ts-project-1": [
        "src/app/[locale]/login/page.tsx",
        "src/features/home/home-screen.tsx",
        "src/features/home/home-component.ts",
        "src/features/home/home.spec.ts",
        "src/app/[locale]/login/page.tsx",
        "src/features/login/login.spec.ts",
        "src/features/login/login-form.ts",
        "tests/fixtures.ts",
        "vitest.config.ts",
        "next.config.ts",
        "next.config.spec.ts",
        "src/lib/util.ts",
    ],
    "ixartz-next-js-boilerplate": [
        ".storybook/preview.ts",
        "next.config.ts",
        "src/components/Hello.tsx",
        "src/libs/DB.ts",
        "src/libs/I18n.ts",
        "src/libs/I18nNavigation.ts",
        "src/libs/Logger.ts",
        "src/proxy.ts",
        "src/templates/BaseTemplate.stories.tsx",
        "src/templates/BaseTemplate.test.tsx",
        "src/utils/Helpers.test.ts",
    ],
    "melzar-nextjs-clean-architecture": [
        "src/app/layout.tsx",
        "src/ui/common/components/layout/ContainerBox/ContainerBox.tsx",
        "src/ui/common/components/layout/TopHeader/TopHeader.stories.tsx",
        "src/ui/common/components/layout/TopNavigation/TopNavigation.tsx",
        "tsconfig.json",
        "src/middleware.ts",
        "src/app/page.tsx",
    ],
}


def golden(name):
    with open(os.path.join(GOLDEN_DIR, name, "golden"), encoding="utf-8") as f:
        return f.read()


def run(command, project):
    tmp = tempfile.mkdtemp(prefix="deslop-qa-")
    proj = os.path.join(tmp, project)
    shutil.copytree(os.path.join(FIXTURES, project), proj)
    env = {**os.environ, "DESLOP_TRANSCRIPT": "1"}
    proc = subprocess.run(
        [BIN, command, proj], capture_output=True, text=True, env=env
    )
    return proc.stdout, proj, tmp


def normalize(transcript, expected, proj=None):
    title = re.search(r"project: (\S+)", expected)
    t = transcript.replace(os.path.expanduser("~"), "~")
    if title and proj:
        # The binary runs on a temp copy; the goldens speak of the fixture.
        t = t.replace(proj, title.group(1))
    if title:
        t = re.sub(
            r"^(\[Title\] 🚀 \w+ing project:).*$",
            lambda m: f"{m.group(1)} {title.group(1)}",
            t,
            flags=re.M,
        )
    t = re.sub(r"^\[Plain\] ⏱  (.*) in \S+$", r"[summary] \1", t, flags=re.M)
    t = t.replace("[Error] ❌ Error: ", "[exit] ")
    # The 'modified' changelog is emitted from a pool; sort each run of it.
    lines, out, i = t.split("\n"), [], 0
    while i < len(lines):
        if lines[i].startswith("[Change] "):
            j = i
            while j < len(lines) and lines[j].startswith("[Change] "):
                j += 1
            out.extend(sorted(lines[i:j]))
            i = j
        else:
            out.append(lines[i])
            i += 1
    return "\n".join(out)


def check_case(project):
    out, proj, tmp = run("check", project)
    return normalize(out, golden(f"check-{project}"), proj), tmp


def baseline_case(project):
    out, proj, tmp = run("baseline", project)
    with open(os.path.join(proj, "deslop", "baseline.yaml"), encoding="utf-8") as f:
        content = f.read()
    return normalize(out, golden(f"baseline-{project}"), proj) + "\n>>> baseline.yaml\n" + content, tmp


def rulebook_error_case(project):
    out, proj, tmp = run("check", project)
    return normalize(out, golden(f"rulebook-error-{project}"), proj), tmp


def fix_case(project):
    # The fix goldens hold only the snapshot of rewritten files.
    _, proj, tmp = run("fix", project)
    snap = ""
    for rel in FIX_SNAPSHOTS[project]:
        with open(os.path.join(proj, *rel.split("/")), encoding="utf-8") as f:
            snap += f"\n\n\n>>> FILE: {rel}\n" + f.read()
    return snap, tmp


CASES = []
for p in PROJECTS:
    CASES.append((f"check-{p}", check_case, p))
    CASES.append((f"baseline-{p}", baseline_case, p))
CASES.append(("rulebook-error-ts-invalid-rulebook-project", rulebook_error_case, "ts-invalid-rulebook-project"))
for p in FIX_SNAPSHOTS:
    CASES.append((f"fix-{p}", fix_case, p))


def main():
    wanted = sys.argv[1:]
    failures = []
    for name, fn, project in CASES:
        if wanted and not any(w in name for w in wanted):
            continue
        try:
            actual, tmp = fn(project)
        except Exception as e:  # noqa: BLE001
            print(f"FAIL {name}: {e}")
            failures.append(name)
            continue
        finally:
            tmp = locals().get("tmp")
        expected = golden(name)
        a = actual.lstrip("\n")
        e = expected.lstrip("\n")
        if a == e:
            print(f"PASS {name}")
        else:
            failures.append(name)
            print(f"FAIL {name}")
            for line in difflib.unified_diff(
                e.splitlines(), a.splitlines(), "golden", "rust", lineterm="", n=1
            ):
                print("   " + line)
    print(f"\n{len(failures)} failing of {len([c for c in CASES if not wanted or any(w in c[0] for w in wanted)])} run")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
