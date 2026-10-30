#!/usr/bin/env python3
"""Unit tests for `anomaly_alerts.py`. Run with `python3 -m unittest test_anomaly_alerts.py`."""

import dataclasses
import json
import math
import random
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from io import StringIO
from unittest import mock

import anomaly_alerts
from anomaly_alerts import (
    METRIC_AGG_NAME,
    HISTOGRAM_AGG_NAME,
    DetectorConfig,
    QuickwitError,
    build_search_request,
    compute_buckets,
    extract_series,
    parse_duration_secs,
    parse_metric,
    score_series,
)


# Short warmup and fast smoothing, for tests on short series.
FAST_CONFIG = DetectorConfig(alpha=0.3, threshold=3.0, warmup=5)


def alternating(low, high, num_values):
    return [float(low if i % 2 == 0 else high) for i in range(num_values)]


class DetectorTest(unittest.TestCase):
    def test_validate(self):
        DetectorConfig().validate()
        for invalid in (
            DetectorConfig(alpha=0.0),
            DetectorConfig(alpha=1.5),
            DetectorConfig(threshold=0.0),
            DetectorConfig(threshold=float("inf")),
            DetectorConfig(warmup=0),
            DetectorConfig(direction="sideways"),
        ):
            with self.assertRaises(ValueError):
                invalid.validate()

    def test_manual_computation(self):
        scores = score_series([2.0, 4.0, 8.0], DetectorConfig(alpha=0.5, threshold=10, warmup=1))
        self.assertIsNone(scores[0])
        self.assertEqual((scores[1].expected, scores[1].std_dev), (2.0, 0.0))
        # After 4.0: mean = 3, variance = 0.5 * (0 + 2 * 1) = 1.
        self.assertEqual((scores[2].expected, scores[2].std_dev, scores[2].z_score), (3.0, 1.0, 5.0))
        self.assertFalse(scores[2].is_anomaly)

    def test_spike_and_drop(self):
        config = FAST_CONFIG
        spike_scores = score_series(alternating(100, 104, 20) + [500.0], config)
        self.assertTrue(all(score is None for score in spike_scores[:5]))
        self.assertTrue(all(not score.is_anomaly for score in spike_scores[5:20]))
        self.assertTrue(spike_scores[20].is_anomaly)
        drop_scores = score_series(alternating(50, 55, 20) + [0.0], config)
        self.assertTrue(drop_scores[20].is_anomaly)
        self.assertLess(drop_scores[20].z_score, -3)

    def test_direction(self):
        values = alternating(100, 104, 20) + [0.0]
        up_config = dataclasses.replace(FAST_CONFIG, direction="up")
        down_config = dataclasses.replace(FAST_CONFIG, direction="down")
        self.assertFalse(score_series(values, up_config)[20].is_anomaly)
        self.assertTrue(score_series(values, down_config)[20].is_anomaly)

    def test_default_config_false_alarm_rate(self):
        rng = random.Random(7)
        noise = [rng.gauss(200, math.sqrt(200)) for _ in range(20_000)]
        scores = [score for score in score_series(noise, DetectorConfig()) if score]
        num_false_alarms = sum(score.is_anomaly for score in scores)
        self.assertLess(num_false_alarms / len(scores), 1 / 1_000)
        for factor in (0.5, 1.5):
            last_score = score_series(noise[:200] + [200 * factor], DetectorConfig())[-1]
            self.assertTrue(last_score.is_anomaly)

    def test_flat_baseline(self):
        scores = score_series([10.0] * 6 + [11.0], DetectorConfig(warmup=2))
        self.assertEqual(scores[5].z_score, 0.0)
        self.assertEqual(scores[6].std_dev, 0.0)
        self.assertTrue(scores[6].is_anomaly)

    def test_missing_values(self):
        scores = score_series([1.0, None, 1.0, None, 1.0], DetectorConfig(warmup=2))
        self.assertEqual(scores[:4], [None] * 4)
        self.assertEqual(scores[4].z_score, 0.0)


class RequestResponseTest(unittest.TestCase):
    def test_parse_duration(self):
        self.assertEqual(parse_duration_secs("30s"), 30)
        self.assertEqual(parse_duration_secs("5m"), 300)
        self.assertEqual(parse_duration_secs("2h"), 7_200)
        self.assertEqual(parse_duration_secs("7d"), 604_800)
        for invalid in ("", "m", "0m", "1.5h", "10ms", "-1m"):
            with self.assertRaises(ValueError):
                parse_duration_secs(invalid)
        self.assertEqual(parse_duration_secs("0s", allow_zero=True), 0)

    def test_parse_metric(self):
        self.assertIsNone(parse_metric(None))
        self.assertEqual(parse_metric("avg:latency"), ("avg", "latency"))
        for invalid in ("avg", "avg:", "median:latency"):
            with self.assertRaises(ValueError):
                parse_metric(invalid)

    def test_build_search_request(self):
        request = build_search_request("svc:a", "ts", "1m", 60, 600, ("avg", "latency"))
        bounds = {"min": 60_000, "max": 599_999}
        self.assertEqual(
            request,
            {
                "query": "svc:a",
                "start_timestamp": 60,
                "end_timestamp": 600,
                "max_hits": 0,
                "aggs": {
                    HISTOGRAM_AGG_NAME: {
                        "date_histogram": {
                            "field": "ts",
                            "fixed_interval": "1m",
                            "min_doc_count": 0,
                            "extended_bounds": bounds,
                            "hard_bounds": bounds,
                        },
                        "aggs": {METRIC_AGG_NAME: {"avg": {"field": "latency"}}},
                    }
                },
            },
        )

    def test_extract_series(self):
        response = {
            "num_hits": 3,
            "errors": [],
            "aggregations": {
                HISTOGRAM_AGG_NAME: {
                    "buckets": [
                        {"key": 60000.0, "doc_count": 3, METRIC_AGG_NAME: {"value": 12.5}},
                        {"key": 120000.0, "doc_count": 0, METRIC_AGG_NAME: {"value": None}},
                    ]
                }
            },
        }
        self.assertEqual(extract_series(response, None), [(60_000, 3, 3.0), (120_000, 0, 0.0)])
        self.assertEqual(
            extract_series(response, ("avg", "latency")),
            [(60_000, 3, 12.5), (120_000, 0, None)],
        )
        self.assertEqual(extract_series({"num_hits": 0, "errors": []}, None), [])
        missing_field_response = {
            "num_hits": 3,
            "errors": [],
            "aggregations": {
                HISTOGRAM_AGG_NAME: {
                    "buckets": [{"key": 0.0, "doc_count": 3, METRIC_AGG_NAME: {"value": None}}]
                }
            },
        }
        with self.assertRaises(QuickwitError):
            extract_series(missing_field_response, ("avg", "nope"))
        with self.assertRaises(QuickwitError):
            extract_series({"num_hits": 0, "errors": ["split failed"]}, None)

    def test_compute_buckets_only_alerts_on_recent_buckets(self):
        values = alternating(100, 104, 10) + [500.0] + alternating(100, 104, 10) + [500.0]
        series = [(i * 60_000, int(value), value) for (i, value) in enumerate(values)]
        buckets = compute_buckets(series, FAST_CONFIG, alert_buckets=1)
        self.assertTrue(buckets[10].is_anomaly)
        self.assertFalse(buckets[10].is_alert)
        self.assertTrue(buckets[-1].is_alert)


class FakeQuickwit(BaseHTTPRequestHandler):
    """Serves a canned search response and records the requests it receives."""

    requests = []
    search_response = {}

    def do_GET(self):
        self.requests.append(("GET", self.path, None))
        self._reply({"index_config": {"doc_mapping": {"timestamp_field": "ts"}}})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.requests.append(("POST", self.path, body))
        self._reply(self.search_response if self.path.endswith("/search") else {})

    def _reply(self, payload):
        data = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


class EndToEndTest(unittest.TestCase):
    def setUp(self):
        self.server = HTTPServer(("127.0.0.1", 0), FakeQuickwit)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.endpoint = f"http://127.0.0.1:{self.server.server_port}"
        FakeQuickwit.requests = []

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()

    def run_main(self, *argv):
        with mock.patch("sys.stdout", new_callable=StringIO) as stdout:
            exit_code = anomaly_alerts.main(["--endpoint", self.endpoint, *argv])
        return exit_code, stdout.getvalue()

    def test_drop_raises_alert_and_calls_webhook(self):
        counts = alternating(100, 104, 11) + [0.0]
        FakeQuickwit.search_response = {
            "num_hits": int(sum(counts)),
            "errors": [],
            "aggregations": {
                HISTOGRAM_AGG_NAME: {
                    "buckets": [
                        {"key": float(i * 60_000), "doc_count": int(count)}
                        for (i, count) in enumerate(counts)
                    ]
                }
            },
        }
        exit_code, stdout = self.run_main(
            "--index", "logs", "--lookback", "12m", "--end", "840", "--delay", "90s",
            "--alpha", "0.3", "--threshold", "3", "--warmup", "5",
            "--webhook", f"{self.endpoint}/hook",
        )
        self.assertEqual(exit_code, anomaly_alerts.EXIT_ALERT)
        self.assertIn("1 alerts", stdout)
        methods_and_paths = [(method, path) for (method, path, _) in FakeQuickwit.requests]
        self.assertEqual(
            methods_and_paths,
            [("GET", "/api/v1/indexes/logs"), ("POST", "/api/v1/logs/search"), ("POST", "/hook")],
        )
        search_body = FakeQuickwit.requests[1][2]
        # 840 - 90 = 750, rounded down to a bucket boundary.
        self.assertEqual((search_body["start_timestamp"], search_body["end_timestamp"]), (0, 720))
        self.assertEqual(
            search_body["aggs"][HISTOGRAM_AGG_NAME]["date_histogram"]["field"], "ts"
        )
        webhook_body = FakeQuickwit.requests[2][2]
        self.assertEqual(len(webhook_body["alerts"]), 1)
        self.assertEqual(webhook_body["alerts"][0]["timestamp_millis"], 660_000)

    def test_no_alert(self):
        FakeQuickwit.search_response = {"num_hits": 0, "errors": []}
        exit_code, _ = self.run_main("--index", "logs", "--timestamp-field", "ts", "--end", "720")
        self.assertEqual(exit_code, anomaly_alerts.EXIT_OK)
        self.assertEqual([method for (method, _, _) in FakeQuickwit.requests], ["POST"])

    def test_invalid_lookback(self):
        with mock.patch("sys.stderr", new_callable=StringIO):
            exit_code, _ = self.run_main("--index", "logs", "--lookback", "5m")
        self.assertEqual(exit_code, anomaly_alerts.EXIT_ERROR)

    def test_backfill_without_delay(self):
        FakeQuickwit.search_response = {"num_hits": 0, "errors": []}
        exit_code, _ = self.run_main(
            "--index", "logs", "--timestamp-field", "ts", "--end", "10800", "--delay", "0s"
        )
        self.assertEqual(exit_code, anomaly_alerts.EXIT_OK)
        search_body = FakeQuickwit.requests[0][2]
        self.assertEqual((search_body["start_timestamp"], search_body["end_timestamp"]), (0, 10800))


if __name__ == "__main__":
    unittest.main()
