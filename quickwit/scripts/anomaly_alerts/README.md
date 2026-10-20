# Anomaly alerts

`anomaly_alerts.py` computes alerts on a Quickwit index from outside Quickwit. It only
uses the standard search API and the Python standard library (3.10+).

It runs one search with a `date_histogram` aggregation (document count, or
`avg`/`sum`/`min`/`max` of a numeric fast field per bucket), then scores each bucket
against an exponentially weighted moving average (EWMA) and variance of the buckets
before it. A bucket is anomalous when `|z_score| >= --threshold`. Only anomalies in the
last `--alert-buckets` buckets raise alerts; older buckets build the baseline.

```bash
# Error-log volume per minute over the last 2 hours, alert on the last complete minute.
./anomaly_alerts.py --index app-logs --query 'severity_text:ERROR' --interval 1m --lookback 2h

# Average checkout latency, alert on increases only, post alerts to a webhook.
./anomaly_alerts.py --index otel-traces-v0_9 --query 'service_name:checkout' \
    --interval 5m --lookback 1d --metric avg:span_duration_millis --direction up \
    --webhook https://hooks.example.com/alerts
```

Run `./anomaly_alerts.py --help` for all options. Exit code is `0` with no alert, `1` when
alerts were raised, `2` on error, so it can be run from cron or a CI-style scheduler.

Behavior worth knowing:

- The window ends on a bucket boundary: the bucket still being filled is never scored.
- Empty buckets are kept, so a source going silent shows up as a drop.
- The baseline is rebuilt from `--lookback` on every run, so `--lookback` must cover
  `--warmup` plus `--alert-buckets` plus one bucket. Longer lookbacks give steadier baselines.
- Anomalies feed the baseline too, so a lasting level shift slowly becomes the new normal.
- A metric with no value in any non-empty bucket (missing or non-numeric field) is an
  error rather than a silent "no alert".
- When `--timestamp-field` is omitted, the index timestamp field is fetched from the index
  metadata, which requires `--index` to be a single index ID.

Tests: `python3 -m unittest test_anomaly_alerts.py` from this directory.
