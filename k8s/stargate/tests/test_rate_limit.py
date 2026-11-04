#!/usr/bin/env python3
"""Check that every Traefik route applies the independent admission limit."""
import unittest
import importlib.util
import sys
from pathlib import Path

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("allowlist_contract", Path(__file__).with_name("test_admin_allowlist.py"))
allowlist = importlib.util.module_from_spec(spec)
spec.loader.exec_module(allowlist)
render, routes = allowlist.render, allowlist.routes

class RateLimitRendering(unittest.TestCase):
    def test_disabled_has_no_middleware(self):
        result = render({})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("-rate-limit", result.stdout)

    def test_every_route_uses_the_limit_before_the_admin_allow_list(self):
        result = render({"traefik": {"rateLimit": {"enabled": True}, "apiHosts": ["one.example.com", "two.example.com"], "grpc": {"enabled": True}, "adminAllowList": {"enabled": True, "sourceRange": ["192.0.2.0/24"]}}})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("  rateLimit:\n    average: 20\n    period: \"1s\"\n    burst: 200", result.stdout)
        parsed = routes(result.stdout)
        self.assertEqual(len(parsed), 5)
        for _, priority, block in parsed:
            self.assertEqual(block.count("- name: check-stargate-rate-limit"), 1)
            if priority == 30:
                self.assertLess(block.index("check-stargate-rate-limit"), block.index("check-stargate-admin-allowlist"))
        self.assertNotIn("redis:", result.stdout)

    def test_router_count_matches_the_documented_scope(self):
        for api_hosts, admin, grpc, expected in [
            ([], False, False, 1),
            (["one.example.com", "two.example.com"], False, False, 2),
            (["one.example.com", "two.example.com"], True, False, 3),
            (["one.example.com", "two.example.com"], False, True, 4),
            (["one.example.com", "two.example.com"], True, True, 5),
        ]:
            with self.subTest(api_hosts=api_hosts, admin=admin, grpc=grpc):
                result = render({"traefik": {
                    "apiHosts": api_hosts,
                    "grpc": {"enabled": grpc},
                    "adminAllowList": {"enabled": admin, "sourceRange": ["192.0.2.0/24"]},
                    "rateLimit": {"enabled": True},
                }})
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(len(routes(result.stdout)), expected)

    def test_depth_is_inherited_but_explicit_zero_is_preserved(self):
        for depth, expected in [(None, 2), (0, 0), (3, 3)]:
            result = render({"traefik": {"adminAllowList": {"ipStrategyDepth": 2}, "rateLimit": {"enabled": True, "ipStrategyDepth": depth}}})
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(f"        depth: {expected}", result.stdout)

    def test_values_are_validated(self):
        for key in ["average", "burst"]:
            for value in [0, -1, 1.5, "invalid", "5"]:
                result = render({"traefik": {"rateLimit": {"enabled": True, key: value}}})
                self.assertNotEqual(result.returncode, 0, (key, value))
                self.assertIn(key, result.stderr)
        result = render({"traefik": {"rateLimit": {"enabled": True}}}, traefik=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires traefik.enabled", result.stderr)

if __name__ == "__main__":
    unittest.main()
