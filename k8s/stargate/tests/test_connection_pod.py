#!/usr/bin/env python3
"""Keep the Helm probe image immutable and the Pod within resource/security bounds."""
import json
from pathlib import Path
import subprocess
import unittest

CHART = Path(__file__).resolve().parents[1]


class ConnectionPod(unittest.TestCase):
    def test_test_pod_has_a_pinned_image_resources_and_non_root_identity(self):
        manifest = subprocess.check_output(['helm', 'template', 'check', str(CHART)], text=True)
        items = json.loads(subprocess.check_output(['ruby', '-ryaml', '-rjson', '-e', 'puts JSON.generate(YAML.load_stream(STDIN.read).compact)'], input=manifest, text=True))
        pod = next(item for item in items if item['kind'] == 'Pod' and item['metadata']['annotations'].get('helm.sh/hook') == 'test')
        container = pod['spec']['containers'][0]
        pin = json.loads((CHART / 'versions.json').read_text())['helmTestImage']
        self.assertRegex(pin['digest'], r'^sha256:[0-9a-f]{64}$')
        self.assertEqual(container['image'], f"{pin['repository']}:{pin['tag']}@{pin['digest']}")
        self.assertIn(pin['digest'], (CHART / 'VERSIONS.md').read_text())
        self.assertEqual(container['resources']['requests'], {'cpu': '10m', 'memory': '16Mi'})
        self.assertEqual(container['resources']['limits'], {'cpu': '100m', 'memory': '32Mi'})
        self.assertTrue(pod['spec']['securityContext']['runAsNonRoot'])
        self.assertGreater(pod['spec']['securityContext']['runAsUser'], 0)
        self.assertGreater(pod['spec']['securityContext']['runAsGroup'], 0)
        self.assertFalse(pod['spec']['automountServiceAccountToken'])
        self.assertFalse(container['securityContext']['allowPrivilegeEscalation'])
        self.assertTrue(container['securityContext']['readOnlyRootFilesystem'])
        self.assertEqual(container['securityContext']['capabilities']['drop'], ['ALL'])
        service = next(item for item in items if item['kind'] == 'Service')
        self.assertFalse(all(pod['metadata']['labels'].get(key) == value for key, value in service['spec']['selector'].items()))


if __name__ == '__main__':
    unittest.main()
