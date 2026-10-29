#!/usr/bin/env python3
"""Verify the migration hooks and their runtime configuration."""

import json
import subprocess
import tempfile
import unittest
from pathlib import Path

CHART = Path(__file__).resolve().parents[1]


def render(overrides, *, cluster=True):
    with tempfile.TemporaryDirectory(prefix="stargate-migrations-test-") as directory:
        values = Path(directory) / "values.json"
        values.write_text(json.dumps(overrides), encoding="utf-8")
        command = ["helm", "template", "check", str(CHART)]
        if cluster:
            command += ["-f", str(CHART / "values-cluster.yaml")]
        command += ["-f", str(values)]
        return subprocess.run(command, capture_output=True, text=True, check=False)


def documents(manifest):
    command = ["ruby", "-ryaml", "-rjson", "-e", "puts JSON.generate(YAML.load_stream(STDIN.read).compact)"]
    result = subprocess.run(command, input=manifest, capture_output=True, text=True, check=True)
    return json.loads(result.stdout)


def resource(items, kind, name):
    return next(item for item in items if item["kind"] == kind and item["metadata"]["name"] == name)


def workload_container(item):
    return item["spec"]["template"]["spec"]["containers"][0]


class MigrationHooks(unittest.TestCase):
    def test_disabled_keeps_runtime_settings_and_emits_no_hooks(self):
        result = render({})
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        self.assertFalse(any(item["metadata"]["name"].endswith("-migrations") for item in items))
        runtime = resource(items, "ConfigMap", "check-stargate-runtime")
        self.assertNotIn("DB_MIGRATE_ON_START", runtime["data"])
        for kind, name in (("Deployment", "check-stargate"), ("Job", "check-stargate-bootstrap")):
            self.assertFalse(any(entry["name"] == "DB_MIGRATE_ON_START" for entry in workload_container(resource(items, kind, name))["env"]))

    def test_hooks_share_runtime_data_and_keep_release_workloads_managed(self):
        overrides = {
            "migrations": {"job": {"enabled": True}},
            "gatewayConfig": {"importOnUpgrade": True},
            "extraVolumes": [{"name": "postgres-ca", "secret": {"secretName": "existing-ca"}}],
            "extraVolumeMounts": [{"name": "postgres-ca", "mountPath": "/ca", "readOnly": True}],
            "extraEnv": [{"name": "POSTGRES_SSLROOTCERT", "value": "/ca/ca.pem"}],
        }
        result = render(overrides)
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        runtime = resource(items, "ConfigMap", "check-stargate-runtime")
        hook = resource(items, "ConfigMap", "check-stargate-migrations-env")
        self.assertEqual(runtime["data"], hook["data"])
        self.assertNotIn("DB_MIGRATE_ON_START", runtime["data"])
        self.assertNotIn("DB_MIGRATE_ON_START", hook["data"])
        self.assertEqual(hook["metadata"]["annotations"]["helm.sh/hook"], "pre-install,pre-upgrade")
        self.assertEqual(hook["metadata"]["annotations"]["helm.sh/hook-weight"], "-10")
        self.assertEqual(hook["metadata"]["annotations"]["helm.sh/hook-delete-policy"], "before-hook-creation,hook-succeeded")
        self.assertFalse(any(item["kind"] == "Secret" and item["metadata"]["name"] == hook["metadata"]["name"] for item in items))
        job = resource(items, "Job", "check-stargate-migrations")
        self.assertEqual(job["metadata"]["annotations"]["helm.sh/hook-weight"], "-5")
        self.assertEqual(job["metadata"]["annotations"]["helm.sh/hook-delete-policy"], "before-hook-creation")
        pod = job["spec"]["template"]["spec"]
        self.assertNotIn("serviceAccountName", pod)
        self.assertFalse(pod["automountServiceAccountToken"])
        container = pod["containers"][0]
        self.assertEqual(container["command"], ["/bin/server", "db", "migrate"])
        self.assertEqual(container["envFrom"][:2], [{"configMapRef": {"name": hook["metadata"]["name"]}}, {"secretRef": {"name": "stargate-runtime"}}])
        self.assertFalse(any(item["name"] in ("POSTGRES_USERNAME", "POSTGRES_PASSWORD") for item in container["env"]))
        self.assertEqual(container["volumeMounts"], overrides["extraVolumeMounts"])
        self.assertEqual(pod["volumes"], overrides["extraVolumes"])
        self.assertIn({"name": "POSTGRES_SSLROOTCERT", "value": "/ca/ca.pem"}, container["env"])
        deployment = resource(items, "Deployment", "check-stargate")
        self.assertIn({"name": "DB_MIGRATE_ON_START", "value": "false"}, workload_container(deployment)["env"])
        selector = deployment["spec"]["selector"]["matchLabels"]
        pod_labels = job["spec"]["template"]["metadata"]["labels"]
        self.assertFalse(all(pod_labels.get(k) == v for k, v in selector.items()))
        for name in ("check-stargate-bootstrap", "check-stargate-config-import"):
            other = resource(items, "Job", name)
            container = workload_container(other)
            self.assertIn({"name": "DB_MIGRATE_ON_START", "value": "false"}, container["env"])
            env_from = container["envFrom"]
            self.assertIn({"configMapRef": {"name": "check-stargate-runtime"}}, env_from)

    def test_external_runtime_configmap_uses_explicit_env_for_all_workloads(self):
        result = render({
            "runtimeConfig": {"create": False, "existingConfigMap": "external-runtime"},
            "migrations": {"job": {"enabled": True}},
            "gatewayConfig": {"importOnUpgrade": True},
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        self.assertFalse(any(item["kind"] == "ConfigMap" and item["metadata"]["name"] == "check-stargate-migrations-env" for item in items))
        job = resource(items, "Job", "check-stargate-migrations")
        self.assertIn({"configMapRef": {"name": "external-runtime"}}, workload_container(job)["envFrom"])
        self.assertIn({"secretRef": {"name": "stargate-runtime"}}, workload_container(job)["envFrom"])
        for kind, name in (("Deployment", "check-stargate"), ("Job", "check-stargate-bootstrap"), ("Job", "check-stargate-config-import")):
            container = workload_container(resource(items, kind, name))
            self.assertIn({"name": "DB_MIGRATE_ON_START", "value": "false"}, container["env"])
            self.assertIn({"configMapRef": {"name": "external-runtime"}}, container["envFrom"])
            self.assertIn({"secretRef": {"name": "stargate-runtime"}}, container["envFrom"])

    def test_created_hook_secret_and_explicit_credentials(self):
        overrides = {
            "migrations": {"job": {"enabled": True, "existingSecret": "ddl-credentials"}},
            "runtimeSecret": {"create": True, "existingSecret": "", "stringData": {
                "POSTGRES_PASSWORD": "app-test-password", "REDIS_URL": "redis://redis:6379"}},
            "extraEnv": [{"name": "POSTGRES_USERNAME", "value": "app-user"}],
        }
        result = render(overrides)
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        runtime = resource(items, "Secret", "check-stargate-runtime")
        hook = resource(items, "Secret", "check-stargate-migrations-env")
        self.assertEqual(runtime["stringData"], hook["stringData"])
        self.assertEqual(hook["metadata"]["annotations"]["helm.sh/hook-weight"], "-10")
        self.assertEqual(hook["metadata"]["annotations"]["helm.sh/hook-delete-policy"], "before-hook-creation,hook-succeeded")
        job = resource(items, "Job", "check-stargate-migrations")
        container = job["spec"]["template"]["spec"]["containers"][0]
        self.assertIn({"secretRef": {"name": "check-stargate-migrations-env"}}, container["envFrom"])
        for key in ("POSTGRES_USERNAME", "POSTGRES_PASSWORD"):
            entries = [entry for entry in container["env"] if entry["name"] == key]
            self.assertEqual(entries, [{"name": key, "valueFrom": {"secretKeyRef": {"name": "ddl-credentials", "key": key}}}])

    def test_external_configmap_with_chart_created_secret(self):
        result = render({
            "runtimeConfig": {"create": False, "existingConfigMap": "external-runtime"},
            "runtimeSecret": {"create": True, "existingSecret": "", "stringData": {
                "POSTGRES_PASSWORD": "app-test-password", "REDIS_URL": "redis://redis:6379"}},
            "migrations": {"job": {"enabled": True}},
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        self.assertFalse(any(item["kind"] == "ConfigMap" and item["metadata"]["name"] == "check-stargate-migrations-env" for item in items))
        resource(items, "Secret", "check-stargate-migrations-env")
        container = workload_container(resource(items, "Job", "check-stargate-migrations"))
        self.assertIn({"configMapRef": {"name": "external-runtime"}}, container["envFrom"])
        self.assertIn({"secretRef": {"name": "check-stargate-migrations-env"}}, container["envFrom"])

    def test_long_names_do_not_select_migration_pod(self):
        result = render({
            "nameOverride": "a" * 52 + "-migrations",
            "migrations": {"job": {"enabled": True}},
            "networkPolicy": {"enabled": True},
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        job = next(item for item in items if item["kind"] == "Job" and item["metadata"]["name"].endswith("-migrations"))
        labels = job["spec"]["template"]["metadata"]["labels"]
        for kind in ("Service", "NetworkPolicy"):
            item = next(item for item in items if item["kind"] == kind)
            selector = item["spec"]["selector"] if kind == "Service" else item["spec"]["podSelector"]["matchLabels"]
            self.assertFalse(all(labels.get(key) == value for key, value in selector.items()))

    def test_invalid_configuration_rejected(self):
        for config in [
            {"runtimeConfig": {"data": {"DB_MIGRATE_ON_START": "true"}}},
            {"runtimeSecret": {"stringData": {"DB_MIGRATE_ON_START": "true"}}},
            {"extraEnv": [{"name": "DB_MIGRATE_ON_START", "value": "true"}]},
        ]:
            config["migrations"] = {"job": {"enabled": True}}
            with self.subTest(config=config):
                result = render(config)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("DB_MIGRATE_ON_START", result.stderr)
        result = render({"migrations": {"job": {"enabled": True}}}, cluster=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cluster-only", result.stderr)


if __name__ == "__main__":
    unittest.main()
