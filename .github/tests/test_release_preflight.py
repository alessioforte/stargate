"""The registry preflight accepts only explicit manifest absence, even in dry runs."""
import io
import os
from pathlib import Path
import textwrap
import unittest
from unittest.mock import patch
import urllib.error

workflow = (Path(__file__).parents[1] / 'workflows/release.yml').read_text()
step = workflow.split('      - name: Refuse existing image or chart versions\n', 1)[1].split('\n      - ', 1)[0]
script = textwrap.dedent(step.split("          python3 - <<'PY'\n", 1)[1].split('\n          PY', 1)[0])


def token():
    return io.BytesIO(b'{"token":"synthetic-registry-token"}')


class PreflightTests(unittest.TestCase):
    def error(self, status):
        error = urllib.error.HTTPError('https://ghcr.io/synthetic', status, 'fixture', {}, io.BytesIO())
        self.addCleanup(error.close)
        return error

    def run_preflight(self, responses):
        env = {'GITHUB_REPOSITORY_OWNER': 'Example', 'VERSION': '0.1.0', 'GITHUB_ACTOR': 'fixture', 'GITHUB_TOKEN': 'synthetic-token', 'DRY_RUN': 'true'}
        with patch.dict(os.environ, env), patch('urllib.request.urlopen', side_effect=responses) as request:
            exec(compile(script, 'release-registry-preflight', 'exec'), {})
            return request.call_count

    def test_both_versions_absent_in_dry_run(self):
        self.assertNotIn('\n        if:', step)
        self.assertEqual(self.run_preflight([token(), self.error(404), token(), self.error(404)]), 4)

    def test_existing_image_stops_release(self):
        with self.assertRaisesRegex(SystemExit, 'Already published: ghcr.io/example/stargate'):
            self.run_preflight([token(), io.BytesIO()])

    def test_existing_chart_stops_release(self):
        with self.assertRaisesRegex(SystemExit, 'Already published: ghcr.io/example/charts/stargate'):
            self.run_preflight([token(), self.error(404), token(), io.BytesIO()])

    def test_authentication_failure_is_not_absence(self):
        with self.assertRaises(urllib.error.HTTPError):
            self.run_preflight([self.error(401)])

    def test_manifest_access_denied_is_not_absence(self):
        with self.assertRaisesRegex(SystemExit, 'HTTP 403'):
            self.run_preflight([token(), self.error(403)])

    def test_network_failure_is_not_absence(self):
        with self.assertRaises(OSError):
            self.run_preflight([OSError('synthetic network failure')])
