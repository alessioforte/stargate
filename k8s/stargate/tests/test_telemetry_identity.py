#!/usr/bin/env python3
"""Check pod identity ownership and Kubernetes environment expansion order."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

CHART = Path(__file__).resolve().parents[1]


def render(overrides=None, cluster=False):
    cmd = ["helm", "template", "identity", str(CHART)]
    if cluster:
        cmd += ["-f", str(CHART / "values-cluster.yaml")]
    with tempfile.TemporaryDirectory(prefix="stargate-telemetry-test-") as directory:
        if overrides:
            values = Path(directory) / "values.json"
            values.write_text(json.dumps(overrides))
            cmd += ["-f", str(values)]
        return subprocess.run(cmd, capture_output=True, text=True, check=False)


class TelemetryIdentity(unittest.TestCase):
    def test_both_profiles_expand_pod_identity_after_downward_api_fields(self):
        for cluster in [False, True]:
            with self.subTest(cluster=cluster):
                result = render(cluster=cluster)
                self.assertEqual(result.returncode, 0, result.stderr)
                deployment = next(doc for doc in result.stdout.split("\n---") if "\nkind: Deployment\n" in doc)
                for key, field in [("POD_NAME", "metadata.name"), ("POD_NAMESPACE", "metadata.namespace")]:
                    self.assertIn(f"- name: {key}\n              valueFrom:\n                fieldRef:\n                  fieldPath: {field}", deployment)
                attrs = deployment.index("- name: OTEL_RESOURCE_ATTRIBUTES")
                self.assertLess(deployment.index("- name: POD_NAME"), attrs)
                self.assertLess(deployment.index("- name: POD_NAMESPACE"), attrs)
                self.assertIn('value: "service.instance.id=$(POD_NAME),k8s.pod.name=$(POD_NAME),k8s.namespace.name=$(POD_NAMESPACE)"', deployment)
                self.assertEqual(deployment.count("- name: OTEL_RESOURCE_ATTRIBUTES"), 1)

    def test_chart_managed_identity_rejects_configmap_and_extra_env_duplicates(self):
        for key in ["POD_NAME", "POD_NAMESPACE", "OTEL_RESOURCE_ATTRIBUTES"]:
            for overrides, path in [
                ({"runtimeConfig": {"data": {key: "conflict"}}}, "runtimeConfig.data"),
                ({"extraEnv": [{"name": key, "value": "conflict"}]}, "extraEnv"),
                ({"extraEnv": [{"name": key, "valueFrom": {"fieldRef": {"fieldPath": "metadata.uid"}}}]}, "extraEnv"),
            ]:
                with self.subTest(key=key, path=path, overrides=overrides):
                    result = render(overrides)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f"{path}.{key} is chart-managed", result.stderr)

    def test_explicit_identity_also_covers_external_envfrom_resources(self):
        result = render({"runtimeConfig": {"create": False, "existingConfigMap": "external-runtime"}, "runtimeSecret": {"existingSecret": "external-secrets"}})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("- name: OTEL_RESOURCE_ATTRIBUTES", result.stdout)
        self.assertIn("name: external-runtime", result.stdout)
        self.assertIn("name: external-secrets", result.stdout)


if __name__ == "__main__":
    unittest.main()
