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

- The window ends `--delay` (default `90s`) before now, rounded down to a bucket boundary.
  Quickwit only makes documents searchable after a commit (`commit_timeout_secs`, 60s by
  default), so a bucket scored too early looks like a drop. Raise `--delay` if your index
  uses a longer commit timeout or ingestion lags; use `--delay 0s` for backfills.
- Empty buckets are kept, so a source going silent shows up as a drop.
- The baseline is rebuilt from `--lookback` on every run, so `--lookback` must cover
  `--warmup` plus `--alert-buckets` plus one bucket.
- Anomalies feed the baseline too, so a lasting level shift slowly becomes the new normal.
- A metric with no value in any non-empty bucket (missing or non-numeric field) is an
  error rather than a silent "no alert".
- When `--timestamp-field` is omitted, the index timestamp field is fetched from the index
  metadata, which requires `--index` to be a single index ID.

## Sizing and tuning

The script never downloads documents: Quickwit computes the histogram and returns one
entry per bucket.

- **Cost per run** grows with the number of matching documents in `--lookback`, because
  every run re-aggregates the whole window. Daily cost is roughly documents per lookback
  window times runs per day.
- **Hard limits:** 65,000 buckets per query (searcher `aggregation_bucket_limit`) and 500 MB
  of aggregation memory (`aggregation_memory_limit`). Scoring 65,000 buckets takes about
  120 ms in Python.
- **How often to run:** once per `--interval`. Running more often re-checks the same
  bucket. Intervals under the commit timeout are not reliable.
- **Documents per bucket:** pick an interval that puts at least 15 to 30 matching
  documents in each bucket. In simulation, a bucket dropping to zero is caught 92 to 99%
  of the time at 15 documents per bucket and 0 to 35% at 5 or fewer. If the baseline is
  all zeros (e.g. rare errors), a single document raises an alert.
- **Lookback:** 150 to 200 buckets is enough for the default `--alpha 0.05`.

False alarms versus detection, simulated on steady traffic (200 documents per bucket,
`--warmup 30`, alerts per day assume 1-minute buckets):

| `--alpha` / `--threshold` | False alerts per day | -50% drop | -25% drop | +50% spike |
|---|---|---|---|---|
| 0.3 / 3 | 57.9 | 100% | 86% | 100% |
| 0.1 / 4 | 1.2 | 100% | 32% | 100% |
| **0.05 / 4 (default)** | **0.2** | 100% | 21% | 100% |
| 0.05 / 5 | ~0 | 100% | 1% | 100% |

Lower the threshold to catch smaller changes at the cost of more false alerts. Real
traffic with daily or weekly cycles raises more false alerts than steady traffic: the
detector has no notion of seasonality.

Tests: `python3 -m unittest test_anomaly_alerts.py` from this directory.
