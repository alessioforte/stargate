#!/usr/bin/env python3
"""Exercise the admin allow-list Helm rendering without a Kubernetes cluster."""

import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path


CHART = Path(__file__).resolve().parents[1]
TRAEFIK_VALUES = CHART / "values-traefik.yaml"


def render(overrides, *, traefik=True):
    with tempfile.TemporaryDirectory(prefix="stargate-helm-test-") as directory:
        values = Path(directory) / "values.json"
        values.write_text(json.dumps(overrides), encoding="utf-8")
        command = ["helm", "template", "check", str(CHART)]
        if traefik:
            command += ["-f", str(TRAEFIK_VALUES)]
        command += ["-f", str(values)]
        return subprocess.run(command, text=True, capture_output=True, check=False)


def routes(manifest):
    ingress = next(part for part in manifest.split("\n---\n") if "kind: IngressRoute\n" in part)
    result = []
    for block in ingress.split("    - kind: Rule\n")[1:]:
        match = re.search(r'^      match: (".*")$', block, re.MULTILINE)
        priority = re.search(r"^      priority: (\d+)$", block, re.MULTILINE)
        if not match or not priority:
            raise AssertionError("route is missing a match or priority")
        result.append((json.loads(match.group(1)), int(priority.group(1)), block))
    return result


class AdminAllowListRendering(unittest.TestCase):
    def test_disabled_preserves_public_routes(self):
        rendered = render({"traefik": {"grpc": {"enabled": True}}})
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        self.assertNotIn("kind: Middleware\n", rendered.stdout)
        rules = routes(rendered.stdout)
        self.assertEqual(len(rules), 4)
        self.assertIn(("Host(`identity.example.com`)", 10), [(rule, priority) for rule, priority, _ in rules])
        self.assertTrue(all("admin-allowlist" not in block for _, _, block in rules))

    def test_enabled_routes_fail_closed_and_preserve_api_grpc(self):
        rendered = render({"traefik": {
            "grpc": {"enabled": True},
            "adminAllowList": {"enabled": True, "sourceRange": ["192.0.2.0/24", "2001:db8::/32", "::/0"], "ipStrategyDepth": 2},
        }})
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        self.assertIn("kind: Middleware\n", rendered.stdout)
        self.assertIn("  ipAllowList:\n", rendered.stdout)
        self.assertIn('      - "192.0.2.0/24"\n', rendered.stdout)
        self.assertIn('      - "2001:db8::/32"\n', rendered.stdout)
        self.assertIn('      - "::/0"\n', rendered.stdout)
        self.assertIn("      depth: 2\n", rendered.stdout)
        rules = routes(rendered.stdout)
        self.assertEqual(len(rules), 5)
        protected = next((rule, block) for rule, priority, block in rules if priority == 30)
        self.assertIn("PathPrefix(`/admin`)", protected[0])
        self.assertIn("PathPrefix(`/stargate`)", protected[0])
        self.assertIn("PathRegexp(`(?i)", protected[0])
        self.assertIn("- name: check-stargate-admin-allowlist", protected[1])
        predicate = protected[0].split(" && ", 1)[1]
        identity = next(rule for rule, priority, _ in rules if priority == 10 and "identity.example.com" in rule)
        self.assertEqual(identity, f"Host(`identity.example.com`) && !{predicate}")
        grpc_identity = next(rule for rule, priority, _ in rules if priority == 20 and "identity.example.com" in rule)
        self.assertIn(f"&& !{predicate}", grpc_identity)
        api = [rule for rule, _, _ in rules if "api.example.com" in rule]
        self.assertEqual(len(api), 2)
        self.assertTrue(all("PathRegexp" not in rule for rule in api))

    def test_zero_depth_uses_remote_address_and_long_names_are_valid(self):
        rendered = render({"fullnameOverride": "a" * 63, "traefik": {
            "adminAllowList": {"enabled": True, "sourceRange": ["192.0.2.1/32"]},
        }})
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        name = "a" * 47 + "-admin-allowlist"
        self.assertEqual(len(name), 63)
        self.assertIn(f"  name: {name}\n", rendered.stdout)
        self.assertIn(f"        - name: {name}\n", rendered.stdout)
        self.assertNotIn("    ipStrategy:\n", rendered.stdout)

    def test_invalid_values_fail_rendering(self):
        cases = [
            {"enabled": True, "sourceRange": []},
            {"enabled": True, "sourceRange": [""]},
            {"enabled": True, "sourceRange": ["192.0.2.1,192.0.2.2"]},
            {"enabled": True, "sourceRange": ["192.0.2.1"], "ipStrategyDepth": -1},
            {"enabled": True, "sourceRange": ["192.0.2.1"], "ipStrategyDepth": "1"},
            {"enabled": True, "sourceRange": ["192.0.2.1"], "unknown": True},
        ]
        for allowlist in cases:
            with self.subTest(allowlist=allowlist):
                self.assertNotEqual(render({"traefik": {"adminAllowList": allowlist}}).returncode, 0)
        without_traefik = render({"traefik": {"adminAllowList": {"enabled": True, "sourceRange": ["192.0.2.1"]}}}, traefik=False)
        self.assertNotEqual(without_traefik.returncode, 0)
        self.assertIn("requires traefik.enabled=true", without_traefik.stderr)


if __name__ == "__main__":
    unittest.main()
