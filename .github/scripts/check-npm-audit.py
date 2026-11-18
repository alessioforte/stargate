#!/usr/bin/env python3
"""Fail on high or critical npm advisories that no documented exception covers.

An exception is accepted only while it covers development-only packages, stays
within its review date and still matches an advisory in every app it lists.
Format and local run: `.github/npm-audit-exceptions.toml`.
"""
import datetime
import json
import subprocess
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

EXCEPTIONS = Path(__file__).resolve().parents[1] / 'npm-audit-exceptions.toml'
FIELDS = ('id', 'apps', 'reason', 'remove_when', 'review_by')
BLOCKING = ('high', 'critical')

UNEXCEPTED = 'high or critical advisory without an exception'
NOT_DEVELOPMENT = 'exception covers a package that is not development-only'
EXPIRED = 'exception past its review date'
UNUSED = 'exception that no longer matches an advisory'


@dataclass(frozen=True)
class Violation:
    app: str
    rule: str
    advisory: str
    detail: str = ''

    def __str__(self):
        detail = f' ({self.detail})' if self.detail else ''
        return f'app {self.app}: {self.rule}: {self.advisory}{detail}'


@dataclass
class Advisory:
    severity: str
    packages: set = field(default_factory=set)


def advisories(report):
    """Map every advisory of an `npm audit --json` report to the packages it affects."""
    found = {}
    for package, vulnerability in report.get('vulnerabilities', {}).items():
        for via in vulnerability['via']:
            if not isinstance(via, dict):
                continue
            advisory = via['url'].rsplit('/', 1)[-1]
            if not advisory.startswith('GHSA-'):
                raise SystemExit(f'Unrecognised advisory URL in npm audit output: {via["url"]}')
            found.setdefault(advisory, Advisory(via['severity'])).packages.add(package)
    return found


def development_only(lock, package):
    """Whether every installed copy of a package is a development dependency."""
    paths = [path for path in lock.get('packages', {}) if path.split('node_modules/')[-1] == package]
    return bool(paths) and all(lock['packages'][path].get('dev') is True for path in paths)


def check(app, report, lock, exceptions, today):
    """Return the violations for one app; an empty list means its audit passes."""
    found = advisories(report)
    excepted = {exception['id'] for exception in exceptions if app in exception['apps']}
    violations = []

    for advisory, details in found.items():
        if details.severity in BLOCKING and advisory not in excepted:
            packages = ', '.join(sorted(details.packages))
            violations.append(Violation(app, UNEXCEPTED, advisory, f'{details.severity}, {packages}'))

    for exception in exceptions:
        advisory = exception['id']
        if app not in exception['apps']:
            continue
        if advisory not in found:
            violations.append(Violation(app, UNUSED, advisory))
            continue
        if exception['review_by'] < today:
            violations.append(Violation(app, EXPIRED, advisory, f'review was due {exception["review_by"]}'))
        violations += [
            Violation(app, NOT_DEVELOPMENT, advisory, package)
            for package in sorted(found[advisory].packages)
            if not development_only(lock, package)
        ]
    return violations


def load_exceptions(path=EXCEPTIONS):
    """Read the documented exceptions, so a typo in the file fails here."""
    entries = tomllib.loads(path.read_text()).get('ignore', [])
    for entry in entries:
        missing = [name for name in FIELDS if name not in entry]
        if missing:
            raise SystemExit(f'{path}: an exception lacks {", ".join(missing)}')
        if not isinstance(entry['apps'], list) or not isinstance(entry['review_by'], datetime.date):
            raise SystemExit(f'{path}: exception {entry["id"]} needs an apps list and an unquoted review_by date')
    return entries


def report_error(report):
    """Return why npm's output is not a complete vulnerability report, or None.

    An unreachable registry answers with a `message`/`error` object and no
    vulnerabilities map: reading it as a clean report would hide the failure.
    """
    if not isinstance(report, dict):
        return 'no JSON object'
    if report.get('auditReportVersion') == 2 and isinstance(report.get('vulnerabilities'), dict) and 'error' not in report:
        return None
    details = (report.get('message'), json.dumps(report['error']) if 'error' in report else '')
    return ' '.join(part for part in details if part) or 'the report has no vulnerabilities map'


def audit_report(app_dir):
    """Run `npm audit --json`, which exits non-zero when it reports advisories."""
    result = subprocess.run(['npm', 'audit', '--json'], cwd=app_dir, capture_output=True, text=True)
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError:
        raise SystemExit(f'npm audit produced no JSON in {app_dir}:\n{result.stderr.strip()}')
    error = report_error(report)
    if error:
        raise SystemExit(f'npm audit could not be read in {app_dir}: {error}\n{result.stderr.strip()}')
    return report


def main(argv):
    if len(argv) != 2:
        raise SystemExit(f'usage: {argv[0]} <app directory>')
    app_dir = Path(argv[1])
    app = app_dir.resolve().name
    lock = json.loads((app_dir / 'package-lock.json').read_text())
    violations = check(app, audit_report(app_dir), lock, load_exceptions(), datetime.date.today())
    for violation in violations:
        print(violation)
    if violations:
        raise SystemExit(1)
    print(f'app {app}: no blocking advisory without an exception')


if __name__ == '__main__':
    main(sys.argv)
