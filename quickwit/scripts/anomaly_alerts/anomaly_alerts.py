#!/usr/bin/env python3
"""Compute anomaly alerts on a Quickwit index.

The script queries the regular search API (`POST /api/v1/<index>/search`) with a
`date_histogram` aggregation, then scores each bucket client-side against an
exponentially weighted moving average (EWMA) and variance of the buckets before it:

    z_score = (value - expected) / std_dev

A bucket is anomalous when `|z_score| >= threshold` (optionally only in one
direction). Only anomalies in the most recent `--alert-buckets` buckets raise
alerts; older buckets serve as history to build the baseline.

The analyzed window always ends on a bucket boundary, so the bucket that is still
being filled is never scored: a half-full bucket would look like a drop.

Exit codes: 0 = no alert, 1 = at least one alert, 2 = error.

Only the Python standard library is required.

Examples:

    # Error-log volume per minute over the last 2 hours, alert on the last minute.
    ./anomaly_alerts.py --index app-logs --query 'severity_text:ERROR' --interval 1m --lookback 2h

    # Average checkout latency, only alert on increases, and post alerts to a webhook.
    ./anomaly_alerts.py --index otel-traces-v0_9 --query 'service_name:checkout' \\
        --interval 5m --lookback 1d --metric avg:span_duration_millis --direction up \\
        --webhook https://hooks.example.com/alerts
"""

import argparse
import json
import math
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import asdict, dataclass

HISTOGRAM_AGG_NAME = "anomaly_histogram"
METRIC_AGG_NAME = "anomaly_metric"
METRIC_TYPES = ("avg", "sum", "min", "max")
DIRECTIONS = ("both", "up", "down")

# A perfectly flat baseline has a variance of zero: without a floor, any deviation
# would yield an infinite z-score. With it, the z-score is huge but finite.
MIN_STD_DEV = 1e-9

INTERVAL_UNITS_SECS = {"s": 1, "m": 60, "h": 3_600, "d": 86_400}

EXIT_OK = 0
EXIT_ALERT = 1
EXIT_ERROR = 2


class QuickwitError(Exception):
    pass


def parse_duration_secs(duration: str) -> int:
    """Parses `30s`, `5m`, `1h`, `7d` into seconds."""
    unit = duration[-1:]
    amount = duration[:-1]
    if unit not in INTERVAL_UNITS_SECS or not amount.isdigit() or int(amount) == 0:
        raise ValueError(
            f"invalid duration `{duration}`: expected a positive integer followed by one of "
            f"{', '.join(INTERVAL_UNITS_SECS)} (e.g. `5m`)"
        )
    return int(amount) * INTERVAL_UNITS_SECS[unit]


@dataclass(frozen=True)
class DetectorConfig:
    alpha: float = 0.3
    threshold: float = 3.0
    warmup: int = 5
    direction: str = "both"

    def validate(self) -> None:
        if not 0.0 < self.alpha <= 1.0:
            raise ValueError(f"`alpha` must be in (0, 1], got {self.alpha}")
        if not (math.isfinite(self.threshold) and self.threshold > 0.0):
            raise ValueError(f"`threshold` must be strictly positive, got {self.threshold}")
        if self.warmup < 1:
            raise ValueError(f"`warmup` must be at least 1, got {self.warmup}")
        if self.direction not in DIRECTIONS:
            raise ValueError(f"`direction` must be one of {DIRECTIONS}, got {self.direction}")


@dataclass(frozen=True)
class Score:
    expected: float
    std_dev: float
    z_score: float
    is_anomaly: bool


def score_series(values: list, config: DetectorConfig) -> list:
    """Scores values in order. Returns one `Score` or `None` per value.

    A point is scored against the baseline built from the points before it, then
    folded into the baseline (anomalies included, so a lasting level shift slowly
    becomes the new normal). `None` values (e.g. an average over an empty bucket)
    are not scored and do not update the baseline. The first `warmup` observed
    points are not scored either.
    """
    mean = 0.0
    variance = 0.0
    num_observed = 0
    scores = []
    for value in values:
        if value is None:
            scores.append(None)
            continue
        if num_observed == 0:
            mean = value
            num_observed = 1
            scores.append(None)
            continue
        deviation = value - mean
        if num_observed >= config.warmup:
            std_dev = math.sqrt(variance)
            z_score = deviation / max(std_dev, MIN_STD_DEV)
            if config.direction == "up":
                is_anomaly = z_score >= config.threshold
            elif config.direction == "down":
                is_anomaly = z_score <= -config.threshold
            else:
                is_anomaly = abs(z_score) >= config.threshold
            scores.append(Score(mean, std_dev, z_score, is_anomaly))
        else:
            scores.append(None)
        # Incremental EWMA update of mean and variance (Finch, 2009).
        increment = config.alpha * deviation
        mean += increment
        variance = (1.0 - config.alpha) * (variance + deviation * increment)
        num_observed += 1
    return scores


def build_search_request(
    query: str,
    timestamp_field: str,
    interval: str,
    start_timestamp: int,
    end_timestamp: int,
    metric: tuple | None,
) -> dict:
    """Builds the body of a Quickwit search request. Timestamps are in seconds, end excluded."""
    bounds = {"min": start_timestamp * 1_000, "max": end_timestamp * 1_000 - 1}
    histogram_agg = {
        "date_histogram": {
            "field": timestamp_field,
            "fixed_interval": interval,
            # Keep empty buckets: a source going silent must show up as a drop.
            "min_doc_count": 0,
            # Return every bucket of the window, including empty ones at the edges.
            "extended_bounds": bounds,
            "hard_bounds": bounds,
        }
    }
    if metric is not None:
        metric_type, metric_field = metric
        histogram_agg["aggs"] = {METRIC_AGG_NAME: {metric_type: {"field": metric_field}}}
    return {
        "query": query,
        "start_timestamp": start_timestamp,
        "end_timestamp": end_timestamp,
        "max_hits": 0,
        "aggs": {HISTOGRAM_AGG_NAME: histogram_agg},
    }


def extract_series(search_response: dict, metric: tuple | None) -> list:
    """Returns `(timestamp_millis, doc_count, value)` tuples, in chronological order."""
    if search_response.get("errors"):
        raise QuickwitError(f"search returned errors: {search_response['errors']}")
    aggregations = search_response.get("aggregations")
    if aggregations is None:
        # Quickwit omits aggregations when no split matched the request.
        return []
    histogram = aggregations.get(HISTOGRAM_AGG_NAME)
    if histogram is None or "buckets" not in histogram:
        raise QuickwitError(f"unexpected aggregation result: {aggregations}")
    series = []
    for bucket in histogram["buckets"]:
        doc_count = bucket["doc_count"]
        if metric is None:
            value = float(doc_count)
        else:
            value = bucket[METRIC_AGG_NAME]["value"]
            if value is not None and not math.isfinite(value):
                raise QuickwitError(f"non-finite metric value in bucket {bucket}")
        series.append((int(bucket["key"]), doc_count, value))
    if metric is not None:
        has_docs = any(doc_count > 0 for (_, doc_count, _) in series)
        has_values = any(value is not None for (_, _, value) in series)
        if has_docs and not has_values:
            # Quickwit returns `null` instead of an error for a missing or non-numeric field.
            # Treating that as "no anomaly" would silently disable the alert.
            raise QuickwitError(
                f"metric `{metric[0]}:{metric[1]}` has no value in any bucket: "
                "check that the field exists and is a numeric fast field"
            )
    return series


@dataclass
class Bucket:
    timestamp_millis: int
    doc_count: int
    value: float | None
    expected: float | None
    std_dev: float | None
    z_score: float | None
    is_anomaly: bool
    is_alert: bool


def compute_buckets(series: list, config: DetectorConfig, alert_buckets: int) -> list:
    """Scores the series. Only anomalies in the last `alert_buckets` buckets become alerts."""
    scores = score_series([value for (_, _, value) in series], config)
    first_alert_index = len(series) - alert_buckets
    buckets = []
    for index, ((timestamp_millis, doc_count, value), score) in enumerate(zip(series, scores)):
        is_anomaly = score is not None and score.is_anomaly
        buckets.append(
            Bucket(
                timestamp_millis=timestamp_millis,
                doc_count=doc_count,
                value=value,
                expected=score.expected if score else None,
                std_dev=score.std_dev if score else None,
                z_score=score.z_score if score else None,
                is_anomaly=is_anomaly,
                is_alert=is_anomaly and index >= first_alert_index,
            )
        )
    return buckets


class QuickwitClient:
    def __init__(self, endpoint: str, timeout_secs: float):
        self.api_root = endpoint.rstrip("/") + "/api/v1"
        self.timeout_secs = timeout_secs

    def _request(self, method: str, path: str, body: dict | None = None) -> dict:
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(
            self.api_root + path,
            data=data,
            method=method,
            headers={"Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout_secs) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            message = error.read().decode(errors="replace")
            raise QuickwitError(f"{method} {path} failed with {error.code}: {message}") from error
        except urllib.error.URLError as error:
            raise QuickwitError(f"{method} {path} failed: {error.reason}") from error

    def search(self, index_id: str, body: dict) -> dict:
        return self._request("POST", f"/{urllib.parse.quote(index_id, safe=',*')}/search", body)

    def default_timestamp_field(self, index_id: str) -> str:
        metadata = self._request("GET", f"/indexes/{urllib.parse.quote(index_id, safe='')}")
        timestamp_field = metadata["index_config"]["doc_mapping"].get("timestamp_field")
        if not timestamp_field:
            raise QuickwitError(
                f"index `{index_id}` has no timestamp field, pass `--timestamp-field`"
            )
        return timestamp_field


def post_webhook(url: str, payload: dict, timeout_secs: float) -> None:
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=timeout_secs):
        pass


def format_timestamp(timestamp_millis: int) -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(timestamp_millis / 1_000))


def format_alert(bucket: Bucket) -> str:
    return (
        f"ALERT {format_timestamp(bucket.timestamp_millis)} value={bucket.value:g} "
        f"expected={bucket.expected:.4g} std_dev={bucket.std_dev:.4g} "
        f"z_score={bucket.z_score:+.2f} doc_count={bucket.doc_count}"
    )


def parse_metric(metric: str | None) -> tuple | None:
    if metric is None:
        return None
    metric_type, _, metric_field = metric.partition(":")
    if metric_type not in METRIC_TYPES or not metric_field:
        raise ValueError(
            f"invalid metric `{metric}`: expected `<{'|'.join(METRIC_TYPES)}>:<field>`"
        )
    return (metric_type, metric_field)


def parse_args(argv: list) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__.split("\n\n")[0],
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--endpoint", default="http://localhost:7280", help="Quickwit URL.")
    parser.add_argument("--index", required=True, help="Index ID (or pattern, see Quickwit docs).")
    parser.add_argument("--query", default="*", help="Query selecting the documents to analyze.")
    parser.add_argument(
        "--timestamp-field",
        help="Datetime fast field to bucket on. Defaults to the index timestamp field "
        "(requires --index to be a single index ID).",
    )
    parser.add_argument("--interval", default="1m", help="Bucket width: `30s`, `5m`, `1h`...")
    parser.add_argument("--lookback", default="2h", help="History analyzed, e.g. `2h`, `7d`.")
    parser.add_argument(
        "--end",
        type=int,
        help="End of the window in seconds since epoch (excluded). Defaults to now. "
        "Rounded down to a bucket boundary.",
    )
    parser.add_argument(
        "--metric",
        help="Metric to analyze per bucket, `<avg|sum|min|max>:<field>`. Defaults to doc count.",
    )
    parser.add_argument("--alpha", type=float, default=0.3, help="EWMA smoothing, in (0, 1].")
    parser.add_argument("--threshold", type=float, default=3.0, help="Minimum |z-score|.")
    parser.add_argument(
        "--warmup", type=int, default=5, help="Buckets used to build the baseline first."
    )
    parser.add_argument("--direction", choices=DIRECTIONS, default="both")
    parser.add_argument(
        "--alert-buckets",
        type=int,
        default=1,
        help="Only anomalies in this many most recent buckets raise alerts.",
    )
    parser.add_argument("--webhook", help="URL receiving a JSON POST when alerts are raised.")
    parser.add_argument("--json", action="store_true", help="Print the full result as JSON.")
    parser.add_argument("--timeout", type=float, default=30.0, help="HTTP timeout in seconds.")
    return parser.parse_args(argv)


def run(args: argparse.Namespace) -> int:
    config = DetectorConfig(args.alpha, args.threshold, args.warmup, args.direction)
    config.validate()
    metric = parse_metric(args.metric)
    interval_secs = parse_duration_secs(args.interval)
    lookback_secs = parse_duration_secs(args.lookback)
    num_buckets = lookback_secs // interval_secs
    if lookback_secs % interval_secs != 0:
        raise ValueError("`lookback` must be a multiple of `interval`")
    if not 1 <= args.alert_buckets <= num_buckets - config.warmup - 1:
        raise ValueError(
            f"`lookback` holds {num_buckets} buckets: not enough for `warmup` ({config.warmup}) "
            f"plus `alert-buckets` ({args.alert_buckets}) plus the first bucket"
        )

    end_timestamp = args.end if args.end is not None else int(time.time())
    # Drop the bucket still being filled.
    end_timestamp -= end_timestamp % interval_secs
    start_timestamp = end_timestamp - lookback_secs

    client = QuickwitClient(args.endpoint, args.timeout)
    timestamp_field = args.timestamp_field or client.default_timestamp_field(args.index)
    search_request = build_search_request(
        args.query, timestamp_field, args.interval, start_timestamp, end_timestamp, metric
    )
    search_response = client.search(args.index, search_request)
    series = extract_series(search_response, metric)
    buckets = compute_buckets(series, config, args.alert_buckets)
    alerts = [bucket for bucket in buckets if bucket.is_alert]

    result = {
        "index": args.index,
        "query": args.query,
        "metric": args.metric or "doc_count",
        "start_timestamp": start_timestamp,
        "end_timestamp": end_timestamp,
        "num_hits": search_response["num_hits"],
        "alerts": [asdict(bucket) for bucket in alerts],
        "buckets": [asdict(bucket) for bucket in buckets],
    }
    if args.json:
        json.dump(result, sys.stdout, indent=2)
        print()
    else:
        print(
            f"{args.index} [{args.metric or 'doc_count'}] {format_timestamp(start_timestamp * 1_000)}"
            f" -> {format_timestamp(end_timestamp * 1_000)}: {len(buckets)} buckets, "
            f"{sum(bucket.is_anomaly for bucket in buckets)} anomalies, {len(alerts)} alerts"
        )
        for bucket in alerts:
            print(format_alert(bucket))
    if alerts and args.webhook:
        webhook_payload = {key: value for key, value in result.items() if key != "buckets"}
        post_webhook(args.webhook, webhook_payload, args.timeout)
    return EXIT_ALERT if alerts else EXIT_OK


def main(argv: list) -> int:
    args = parse_args(argv)
    try:
        return run(args)
    except (ValueError, QuickwitError, urllib.error.URLError) as error:
        print(f"error: {error}", file=sys.stderr)
        return EXIT_ERROR


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
