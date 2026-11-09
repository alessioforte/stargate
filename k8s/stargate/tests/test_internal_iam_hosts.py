#!/usr/bin/env python3
"""Internal IAM names must be explicit and must not become public routes."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

CHART = Path(__file__).resolve().parents[1]


def render(overrides, traefik=True):
    with tempfile.TemporaryDirectory(prefix='stargate-iam-hosts-') as directory:
        values = Path(directory) / 'values.json'
        values.write_text(json.dumps(overrides))
        command = ['helm', 'template', 'check', str(CHART)]
        if traefik:
            command += ['-f', str(CHART / 'values-traefik.yaml')]
        return subprocess.run(command + ['-f', str(values)], capture_output=True, text=True)


def documents(manifest):
    return json.loads(subprocess.check_output(['ruby', '-ryaml', '-rjson', '-e', 'puts JSON.generate(YAML.load_stream(STDIN.read).compact)'], input=manifest, text=True))


def iam_hosts(items):
    deployment = next(item for item in items if item['kind'] == 'Deployment')
    return next(env['value'] for env in deployment['spec']['template']['spec']['containers'][0]['env'] if env['name'] == 'IAM_HOSTS')


class InternalIamHosts(unittest.TestCase):
    def test_default_preserves_public_identity_host_only(self):
        result = render({})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(iam_hosts(documents(result.stdout)), 'identity.example.com')

    def test_unset_list_preserves_existing_values(self):
        result = render({'traefik': {'internalIamHosts': None}})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(iam_hosts(documents(result.stdout)), 'identity.example.com')

    def test_internal_service_names_work_with_external_runtime_config(self):
        result = render({'runtimeConfig': {'create': False, 'existingConfigMap': 'external'}, 'traefik': {'internalIamHosts': ['Stargate.Identity.Svc.Cluster.Local', 'stargate.identity.svc']}})
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        self.assertEqual(iam_hosts(items), 'identity.example.com,stargate.identity.svc.cluster.local,stargate.identity.svc')
        baseline = documents(render({}).stdout)
        for kind in ['IngressRoute', 'NetworkPolicy']:
            self.assertEqual([item for item in items if item['kind'] == kind], [item for item in baseline if item['kind'] == kind])

    def test_internal_hosts_are_emitted_without_a_trailing_dot(self):
        for external_config in (False, True):
            with self.subTest(external_config=external_config):
                overrides = {'traefik': {'internalIamHosts': ['Stargate.Identity.Svc.Cluster.Local.', 'stargate.identity.svc.']}}
                if external_config:
                    overrides['runtimeConfig'] = {'create': False, 'existingConfigMap': 'external'}
                result = render(overrides)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(iam_hosts(documents(result.stdout)), 'identity.example.com,stargate.identity.svc.cluster.local,stargate.identity.svc')

    def test_invalid_names_fail_rendering(self):
        for name in ['', '*.svc.cluster.local', 'https://stargate.identity.svc', 'stargate.identity.svc:5050', 'stargate.identity.svc/path', 'host?query', 'host#fragment', 'user@host', 'bad host', 'host,other', 'a..b', '-host', 'host-', 'host_name', 'x' * 64 + '.svc', 'x.' * 127 + 'svc']:
            with self.subTest(name=name):
                self.assertNotEqual(render({'traefik': {'internalIamHosts': [name]}}).returncode, 0)

    def test_duplicate_or_public_api_hosts_fail_rendering(self):
        for names in [['IDENTITY.EXAMPLE.COM'], ['API.EXAMPLE.COM'], ['API.EXAMPLE.COM.'], ['IDENTITY.EXAMPLE.COM.'], ['stargate.svc', 'stargate.svc.'], ['stargate.svc', 'STARGATE.SVC'], ['stargate.svc', 'stargate.svc']]:
            with self.subTest(names=names):
                result = render({'traefik': {'internalIamHosts': names}})
                self.assertNotEqual(result.returncode, 0)

    def test_requires_traefik_and_managed_overrides_stay_rejected(self):
        result = render({'traefik': {'internalIamHosts': ['stargate.svc']}}, traefik=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('requires traefik.enabled=true', result.stderr)
        for overrides in [{'runtimeConfig': {'data': {'IAM_HOSTS': 'other'}}}, {'extraEnv': [{'name': 'IAM_HOSTS', 'value': 'other'}]}]:
            self.assertNotEqual(render(overrides).returncode, 0)


if __name__ == '__main__':
    unittest.main()
