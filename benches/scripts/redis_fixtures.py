"""Disposable fixtures shared by the Redis bench scripts.

Stargate resolves `.stargate/transactional` from its working directory at
startup and requires one template and one subject per supported locale
(`crates/smtp/src/smtp.rs`). The bench servers run in a temporary directory, so
the assets are written there. Template ids are the filenames in
`apps/mail/emails`; the bench never sends email, so the rendered content is
empty.
"""

import json
from pathlib import Path

LOCALES = ("en", "it")


def write_transactional_templates(directory):
    root = Path(__file__).resolve().parents[2]
    templates = sorted(path.stem for path in (root / "apps/mail/emails").glob("*.tsx"))
    if not templates:
        raise AssertionError("no transactional email templates found")
    target = Path(directory) / ".stargate" / "transactional"
    target.mkdir(parents=True, exist_ok=True)
    subjects = {}
    for locale in LOCALES:
        subjects[locale] = {}
        for template in templates:
            subjects[locale][template] = template
            (target / f"{template}.{locale}.html").write_text("")
    (target / "subjects.json").write_text(json.dumps(subjects) + "\n")
