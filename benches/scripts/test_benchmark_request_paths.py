#!/usr/bin/env python3
"""Unit tests for the Redis round-trip cap check."""

import unittest

import benchmark_request_paths as bench


def result(name: str, round_trips: int) -> dict:
    return {"paths": [{"path": name, "redis_round_trips": round_trips}]}


class RoundTripCapsTest(unittest.TestCase):
    def test_a_path_at_its_cap_passes(self):
        bench.enforce_round_trip_caps(result("session", 4))

    def test_a_path_over_its_cap_fails(self):
        with self.assertRaises(SystemExit) as error:
            bench.enforce_round_trip_caps(result("session", 5))
        self.assertIn("session: 5 > 4", str(error.exception))

    def test_every_path_has_a_cap(self):
        for name in ("anonymous", "rate_limit", "api_key", "quota", "session", "oauth"):
            self.assertIn(name, bench.ROUND_TRIP_CAPS)

    def test_unmeasured_and_unknown_paths_are_ignored(self):
        bench.enforce_round_trip_caps(
            {"paths": [{"path": "unknown", "redis_round_trips": 99}]}
        )
        bench.enforce_round_trip_caps({"paths": [{"path": "session"}]})


if __name__ == "__main__":
    unittest.main()
