"""Audit failures block dependency changes while remaining visible on other PRs."""
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


workflow = (Path(__file__).parents[1] / 'workflows/ci.yml').read_text()
step = workflow.split('      - name: Check dependency audits\n', 1)[1]
script = textwrap.dedent(step.split('        run: |\n', 1)[1])
path_step = workflow.split('      - name: Compare dependency paths\n', 1)[1].split('\n  release-tests:', 1)[0]
path_script = textwrap.dedent(path_step.split('        run: |\n', 1)[1])


class AuditSummaryTests(unittest.TestCase):
    def run_summary(self, enforce='true', changes='success', rust='success', npm='success'):
        return subprocess.run(
            ['bash', '-e', '-o', 'pipefail', '-c', script],
            env={**os.environ, 'ENFORCE': enforce, 'CHANGES': changes, 'RUST': rust, 'NPM': npm},
            capture_output=True,
            text=True,
        )

    def test_successful_audits_pass(self):
        self.assertEqual(self.run_summary().returncode, 0)

    def test_dependency_changes_require_both_audits_to_pass(self):
        for status in ('failure', 'cancelled', 'skipped', ''):
            for job in ('rust', 'npm'):
                with self.subTest(job=job, status=status):
                    self.assertNotEqual(self.run_summary(**{job: status}).returncode, 0)

    def test_unrelated_changes_report_audit_failures_without_blocking(self):
        result = self.run_summary(enforce='false', rust='failure', npm='failure')
        self.assertEqual(result.returncode, 0)
        self.assertIn('informational', result.stdout)
        self.assertIn('Rust: failure, npm: failure', result.stdout)

    def test_path_detection_must_succeed_even_for_unrelated_changes(self):
        for status in ('failure', 'cancelled', 'skipped', ''):
            with self.subTest(status=status):
                self.assertNotEqual(self.run_summary(enforce='false', changes=status).returncode, 0)

    def test_missing_or_invalid_path_output_fails_closed(self):
        for output in ('', 'unknown'):
            with self.subTest(output=output):
                self.assertNotEqual(self.run_summary(enforce=output).returncode, 0)

    def test_weekly_and_reusable_release_runs_always_enforce_audits(self):
        # Reusable workflows inherit the caller's event (tag push or manual release).
        for event in ('schedule', 'push', 'workflow_dispatch'):
            with self.subTest(event=event), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / 'output'
                result = subprocess.run(
                    ['bash', '-e', '-o', 'pipefail', '-c', path_script],
                    env={**os.environ, 'EVENT_NAME': event, 'GITHUB_OUTPUT': str(output)},
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output.read_text(), 'matched=true\n')


if __name__ == '__main__':
    unittest.main()
