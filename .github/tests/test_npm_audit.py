"""Documented npm exceptions: development packages only, live review dates, no leftovers."""
import contextlib
import datetime
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('check_npm_audit', Path(__file__).parents[1] / 'scripts/check-npm-audit.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

ADVISORY = 'GHSA-aaaa-bbbb-cccc'
TODAY = datetime.date(2026, 11, 18)
REVIEW_BY = datetime.date(2026, 12, 17)


def report(*advisories):
    """Build an `npm audit --json` report from (package, advisory, severity) triples."""
    vulnerabilities = {}
    for package, advisory, severity in advisories:
        vulnerabilities[package] = {
            'name': package,
            'severity': severity,
            'isDirect': False,
            'via': [{'name': package, 'severity': severity, 'url': f'https://github.com/advisories/{advisory}', 'range': '<1.0.0'}],
            'effects': [],
            'range': '*',
            'nodes': [f'node_modules/{package}'],
            'fixAvailable': False,
        }
    return {'auditReportVersion': 2, 'vulnerabilities': vulnerabilities}


def lockfile(*packages):
    """Build a package lock from (name, development) pairs."""
    return {'lockfileVersion': 3, 'packages': {f'node_modules/{name}': ({'dev': True} if dev else {}) for name, dev in packages}}


class NpmAuditTests(unittest.TestCase):
    def exception(self, **overrides):
        entry = {'id': ADVISORY, 'apps': ['auth'], 'reason': 'fixture', 'remove_when': 'fixture', 'review_by': REVIEW_BY}
        entry.update(overrides)
        return entry

    def check(self, audit, dependencies, exceptions, app='auth', today=TODAY):
        lock = lockfile(*dependencies)
        return [(violation.rule, violation.advisory, violation.app) for violation in module.check(app, audit, lock, exceptions, today)]

    def test_excepted_development_advisory(self):
        audit = report(('picomatch', ADVISORY, 'high'))
        self.assertEqual(self.check(audit, [('picomatch', True)], [self.exception()]), [])

    def test_unexcepted_advisory(self):
        audit = report(('picomatch', ADVISORY, 'high'))
        self.assertEqual(self.check(audit, [('picomatch', True)], []), [(module.UNEXCEPTED, ADVISORY, 'auth')])

    def test_exception_for_a_shipped_package(self):
        audit = report(('picomatch', ADVISORY, 'high'))
        violations = self.check(audit, [('picomatch', False)], [self.exception()])
        self.assertEqual(violations, [(module.NOT_DEVELOPMENT, ADVISORY, 'auth')])

    def test_exception_without_a_lockfile_entry(self):
        audit = report(('picomatch', ADVISORY, 'high'))
        self.assertEqual(self.check(audit, [], [self.exception()]), [(module.NOT_DEVELOPMENT, ADVISORY, 'auth')])

    def test_exception_past_its_review_date(self):
        audit = report(('picomatch', ADVISORY, 'high'))
        exception = self.exception(review_by=datetime.date(2026, 11, 17))
        self.assertEqual(self.check(audit, [('picomatch', True)], [exception]), [(module.EXPIRED, ADVISORY, 'auth')])

    def test_exception_without_a_matching_advisory(self):
        self.assertEqual(self.check(report(), [], [self.exception()]), [(module.UNUSED, ADVISORY, 'auth')])

    def test_exception_only_covers_the_listed_apps(self):
        audit = report(('picomatch', ADVISORY, 'high'))
        self.assertEqual(self.check(audit, [('picomatch', True)], [self.exception(apps=['console'])]), [(module.UNEXCEPTED, ADVISORY, 'auth')])

    def test_moderate_advisory_needs_no_exception(self):
        audit = report(('next', ADVISORY, 'moderate'))
        self.assertEqual(self.check(audit, [('next', False)], []), [])

    def test_cli_reports_the_rule_and_the_advisory(self):
        audit = report(('picomatch', ADVISORY, 'critical'))
        with tempfile.TemporaryDirectory() as directory:
            app_dir = Path(directory) / 'auth'
            app_dir.mkdir()
            (app_dir / 'package-lock.json').write_text(json.dumps(lockfile(('picomatch', True))))
            output = io.StringIO()
            with patch.object(module, 'audit_report', return_value=audit), patch.object(module, 'load_exceptions', return_value=[]), contextlib.redirect_stdout(output):
                with self.assertRaisesRegex(SystemExit, '1'):
                    module.main(['check-npm-audit', str(app_dir)])
        self.assertEqual(output.getvalue().strip(), f'app auth: {module.UNEXCEPTED}: {ADVISORY} (critical, picomatch)')

    def test_shipped_exceptions_are_complete(self):
        for exception in module.load_exceptions():
            self.assertTrue(exception['reason'].strip())
            self.assertTrue(exception['remove_when'].strip())
