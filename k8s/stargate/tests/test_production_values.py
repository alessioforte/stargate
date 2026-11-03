#!/usr/bin/env python3
"""Render the production overlay and check its hardening without a cluster."""

import json
import subprocess
import tempfile
import unittest
from pathlib import Path


CHART = Path(__file__).resolve().parents[1]
TRAEFIK_VALUES = CHART / "values-traefik.yaml"
PRODUCTION_VALUES = CHART / "values-production.yaml"


def render(overrides=None):
    command = ["helm", "template", "check", str(CHART), "-f", str(TRAEFIK_VALUES), "-f", str(PRODUCTION_VALUES)]
    with tempfile.TemporaryDirectory(prefix="stargate-production-test-") as directory:
        if overrides is not None:
            values = Path(directory) / "values.json"
            values.write_text(json.dumps(overrides), encoding="utf-8")
            command += ["-f", str(values)]
        return subprocess.run(command, text=True, capture_output=True, check=False)


def document(manifest, kind, name_suffix=""):
    for part in manifest.split("\n---\n"):
        if f"\nkind: {kind}\n" in f"\n{part}" and f"name: check-stargate{name_suffix}\n" in part:
            return part
    raise AssertionError(f"{kind} check-stargate{name_suffix} not rendered")


class ProductionOverlay(unittest.TestCase):
    def setUp(self):
        rendered = render()
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        self.manifest = rendered.stdout

    def test_runtime_settings_are_hardened(self):
        config = document(self.manifest, "ConfigMap", "-runtime")
        for line in [
            'POSTGRES_SSLMODE: "verify-full"',
            'POSTGRES_SSLROOTCERT: "/etc/stargate/postgres-ca/ca.crt"',
            'POSTGRES_CREATE_DATABASE: "false"',
            'REDIS_CA_FILE: "/etc/stargate/redis-ca/ca.crt"',
            'MFA_REQUIRED_FOR_SUPER_ADMIN: "true"',
            'MFA_ADMIN_STEP_UP_REQUIRED: "true"',
            'SIGNUP_MODE: "invite"',
            'LOG_FORMAT: "json"',
            'AUDIT_STREAM_CONSUMER_GROUPS: "audit-archiver"',
        ]:
            self.assertIn(line, config)

    def test_image_is_the_released_cluster_image(self):
        # release.yml tags the cluster image with the bare version.
        deployment = document(self.manifest, "Deployment")
        self.assertIn('image: "ghcr.io/alessioforte/stargate:0.1.0"\n', deployment)

    def test_admin_allow_list_and_migration_job_are_enabled(self):
        self.assertIn("kind: Middleware\n", self.manifest)
        self.assertIn("  ipAllowList:\n", self.manifest)
        self.assertIn("kind: Job\n", self.manifest)
        self.assertIn('"/bin/server", "db", "migrate"', self.manifest)
        deployment = document(self.manifest, "Deployment")
        self.assertIn("- name: DB_MIGRATE_ON_START\n", deployment)

    def test_replicas_spread_and_survive_one_disruption(self):
        deployment = document(self.manifest, "Deployment")
        self.assertIn("  replicas: 3\n", deployment)
        self.assertEqual(deployment.count("topologyKey:"), 2)
        # The chart fills in the release selector for constraints without one.
        self.assertEqual(deployment.count("app.kubernetes.io/instance: check\n              app.kubernetes.io/name: stargate"), 2)
        pdb = document(self.manifest, "PodDisruptionBudget")
        self.assertIn("  maxUnavailable: 1\n", pdb)
        self.assertNotIn("minAvailable", pdb)

    def test_ca_bundles_reach_server_and_migrations(self):
        for kind, suffix in [("Deployment", ""), ("Job", "-migrations")]:
            workload = document(self.manifest, kind, suffix)
            self.assertIn("mountPath: /etc/stargate/postgres-ca\n", workload)
            self.assertIn("mountPath: /etc/stargate/redis-ca\n", workload)

    def test_explicit_spread_selector_is_kept(self):
        selector = {"matchLabels": {"tier": "identity"}}
        rendered = render({"topologySpreadConstraints": [{
            "maxSkew": 1,
            "topologyKey": "kubernetes.io/hostname",
            "whenUnsatisfiable": "DoNotSchedule",
            "labelSelector": selector,
        }]})
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        deployment = document(rendered.stdout, "Deployment")
        self.assertIn("tier: identity\n", deployment)
        self.assertEqual(deployment.count("topologyKey:"), 1)


if __name__ == "__main__":
    unittest.main()
