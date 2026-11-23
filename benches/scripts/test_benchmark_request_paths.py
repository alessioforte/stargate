#!/usr/bin/env python3
"""Unit tests for the Redis round-trip cap check."""

import unittest

import benchmark_request_paths as bench

ALL_PATHS = (
    "anonymous",
    "rate_limit",
    "api_key",
    "quota",
    "durable_quota",
    "session",
    "oauth",
)


def result(name: str, round_trips) -> dict:
    return {"paths": [{"path": name, "redis_round_trips": round_trips}]}


def all_paths(round_trips: int = 1) -> dict:
    return {"paths": [{"path": name, "redis_round_trips": round_trips} for name in ALL_PATHS]}


class RoundTripCapsTest(unittest.TestCase):
    def test_a_path_at_its_cap_passes(self):
        bench.enforce_round_trip_caps(result("session", 4), ["session"])

    def test_a_path_over_its_cap_fails(self):
        with self.assertRaises(SystemExit) as error:
            bench.enforce_round_trip_caps(result("session", 5), ["session"])
        self.assertIn("session: 5 > 4", str(error.exception))

    def test_a_missing_selected_path_fails(self):
        with self.assertRaises(SystemExit) as error:
            bench.enforce_round_trip_caps({"paths": []}, ["session"])
        self.assertIn("session: no integer", str(error.exception))

    def test_a_null_count_fails(self):
        with self.assertRaises(SystemExit):
            bench.enforce_round_trip_caps(result("session", None), ["session"])

    def test_every_path_has_a_cap(self):
        for name in ALL_PATHS:
            self.assertIn(name, bench.ROUND_TRIP_CAPS)

    def test_the_default_selection_requires_every_capped_path(self):
        bench.enforce_round_trip_caps(all_paths())
        with self.assertRaises(SystemExit) as error:
            bench.enforce_round_trip_caps(result("session", 4))
        self.assertIn("anonymous: no integer", str(error.exception))

    def test_unknown_paths_are_ignored(self):
        bench.enforce_round_trip_caps(
            {"paths": [{"path": "unknown", "redis_round_trips": 99}]}, ["unknown"]
        )


if __name__ == "__main__":
    unittest.main()
