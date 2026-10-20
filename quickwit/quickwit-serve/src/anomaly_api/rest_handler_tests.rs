// Copyright 2021-Present Datadog, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use quickwit_proto::search::SplitSearchError;
use quickwit_search::MockSearchService;
use serde_json::{Value as JsonValue, json};
use tantivy::aggregation::metric::SingleMetricResult;

use super::*;
use crate::recover_fn;

fn anomaly_handler(
    mock_search_service: MockSearchService,
) -> impl Filter<Extract = (impl warp::Reply,), Error = Rejection> + Clone {
    anomaly_detection_post_handler(Arc::new(mock_search_service)).recover(recover_fn)
}

fn histogram_postcard(buckets: Vec<(f64, u64, Option<Option<f64>>)>) -> Vec<u8> {
    let bucket_entries: Vec<BucketEntry> = buckets
        .into_iter()
        .map(|(timestamp_millis, doc_count, metric_opt)| {
            let sub_aggregation = match metric_opt {
                Some(value) => vec![(
                    METRIC_AGG_NAME.to_string(),
                    AggregationResult::MetricResult(MetricResult::Average(SingleMetricResult {
                        value,
                    })),
                )],
                None => Vec::new(),
            };
            BucketEntry {
                key_as_string: None,
                key: Key::F64(timestamp_millis),
                doc_count,
                sub_aggregation: AggregationResults(sub_aggregation),
            }
        })
        .collect();
    let aggregation_results = AggregationResults(vec![(
        HISTOGRAM_AGG_NAME.to_string(),
        AggregationResult::BucketResult(BucketResult::Histogram {
            buckets: BucketEntries::Vec(bucket_entries),
        }),
    )]);
    postcard::to_stdvec(&aggregation_results).unwrap()
}

fn parse_request(body: JsonValue) -> AnomalyDetectionRequest {
    serde_json::from_value(body).unwrap()
}

#[test]
fn test_build_search_request_doc_count() {
    let request = parse_request(json!({
        "query": "service_name:checkout",
        "timestamp_field": "span_start_timestamp_nanos",
        "interval": "1m",
        "start_timestamp": 60,
        "end_timestamp": 600,
    }));
    let search_request = build_search_request(vec!["otel-traces".to_string()], &request).unwrap();
    assert_eq!(search_request.max_hits, 0);
    assert_eq!(search_request.start_timestamp, Some(60));
    assert_eq!(search_request.end_timestamp, Some(600));
    let aggregation_request: JsonValue =
        serde_json::from_str(search_request.aggregation_request.as_ref().unwrap()).unwrap();
    let expected_bounds = json!({ "min": 60_000, "max": 599_999 });
    assert_eq!(
        aggregation_request,
        json!({
            HISTOGRAM_AGG_NAME: {
                "date_histogram": {
                    "field": "span_start_timestamp_nanos",
                    "fixed_interval": "1m",
                    "min_doc_count": 0,
                    "extended_bounds": expected_bounds,
                    "hard_bounds": expected_bounds,
                }
            }
        })
    );
    // The generated request must be accepted by tantivy.
    serde_json::from_value::<tantivy::aggregation::agg_req::Aggregations>(aggregation_request)
        .unwrap();
}

#[test]
fn test_build_search_request_with_metric() {
    let request = parse_request(json!({
        "timestamp_field": "span_start_timestamp_nanos",
        "interval": "30s",
        "metric": { "type": "avg", "field": "span_duration_millis" },
    }));
    assert_eq!(request.query, "*");
    let search_request = build_search_request(vec!["otel-traces".to_string()], &request).unwrap();
    let aggregation_request: JsonValue =
        serde_json::from_str(search_request.aggregation_request.as_ref().unwrap()).unwrap();
    assert_eq!(
        aggregation_request[HISTOGRAM_AGG_NAME]["aggs"],
        json!({ METRIC_AGG_NAME: { "avg": { "field": "span_duration_millis" } } })
    );
    assert!(aggregation_request[HISTOGRAM_AGG_NAME]["date_histogram"]["extended_bounds"].is_null());
    serde_json::from_value::<tantivy::aggregation::agg_req::Aggregations>(aggregation_request)
        .unwrap();
}

#[test]
fn test_build_search_request_invalid() {
    let invalid_bodies = [
        json!({ "timestamp_field": "ts", "interval": "1m", "alpha": 0.0 }),
        json!({ "timestamp_field": "ts", "interval": "1m", "threshold": -1.0 }),
        json!({ "timestamp_field": "ts", "interval": "1m", "warmup": 0 }),
        json!({ "timestamp_field": "ts", "interval": "1m", "start_timestamp": 10, "end_timestamp": 10 }),
    ];
    for body in invalid_bodies {
        let request = parse_request(body);
        let error = build_search_request(vec!["index".to_string()], &request).unwrap_err();
        assert!(matches!(error, SearchError::InvalidArgument(_)));
    }
}

#[test]
fn test_build_response_flags_metric_spike() {
    let request = parse_request(json!({
        "timestamp_field": "ts",
        "interval": "1m",
        "metric": { "type": "avg", "field": "duration" },
        "warmup": 3,
    }));
    let mut buckets: Vec<(f64, u64, Option<Option<f64>>)> = (0..10)
        .map(|i| {
            let latency = if i % 2 == 0 { 20.0 } else { 22.0 };
            (i as f64 * 60_000.0, 10, Some(Some(latency)))
        })
        .collect();
    buckets.push((600_000.0, 0, Some(None)));
    buckets.push((660_000.0, 10, Some(Some(250.0))));
    let search_response = SearchResponse {
        num_hits: 110,
        aggregation_postcard: Some(histogram_postcard(buckets)),
        ..Default::default()
    };
    let response = build_response(search_response, &request).unwrap();
    assert_eq!(response.num_hits, 110);
    assert_eq!(response.buckets.len(), 12);
    assert_eq!(response.num_anomalies, 1);
    assert_eq!(response.buckets[1].timestamp_millis, 60_000);
    assert!(response.buckets[0].z_score.is_none());
    let empty_bucket = &response.buckets[10];
    assert_eq!(empty_bucket.value, None);
    assert_eq!(empty_bucket.z_score, None);
    assert!(!empty_bucket.is_anomaly);
    let spike_bucket = &response.buckets[11];
    assert_eq!(spike_bucket.value, Some(250.0));
    assert!(spike_bucket.is_anomaly);
    assert!(spike_bucket.z_score.unwrap() > 3.0);
}

#[test]
fn test_build_response_fails_on_split_errors() {
    let request = parse_request(json!({ "timestamp_field": "ts", "interval": "1m" }));
    let search_response = SearchResponse {
        aggregation_postcard: Some(histogram_postcard(Vec::new())),
        failed_splits: vec![SplitSearchError {
            error: "boom".to_string(),
            split_id: "split-1".to_string(),
            retryable_error: false,
        }],
        ..Default::default()
    };
    build_response(search_response, &request).unwrap_err();
}

#[test]
fn test_build_response_no_aggregation() {
    let request = parse_request(json!({ "timestamp_field": "ts", "interval": "1m" }));
    let response = build_response(SearchResponse::default(), &request).unwrap();
    assert!(response.buckets.is_empty());
    assert_eq!(response.num_anomalies, 0);
}

#[tokio::test]
async fn test_anomaly_detection_route_doc_count_drop() {
    let mut mock_search_service = MockSearchService::new();
    mock_search_service
        .expect_root_search()
        .returning(|search_request| {
            assert_eq!(search_request.index_id_patterns, vec!["logs".to_string()]);
            let mut buckets: Vec<(f64, u64, Option<Option<f64>>)> = (0..10)
                .map(|i| (i as f64 * 60_000.0, 100 + (i % 3), None))
                .collect();
            buckets.push((600_000.0, 0, None));
            Ok(SearchResponse {
                num_hits: 1_000,
                aggregation_postcard: Some(histogram_postcard(buckets)),
                ..Default::default()
            })
        });
    let response = warp::test::request()
        .method("POST")
        .path("/logs/anomalies")
        .json(&json!({ "timestamp_field": "timestamp", "interval": "1m" }))
        .reply(&anomaly_handler(mock_search_service))
        .await;
    assert_eq!(response.status(), 200);
    let body: JsonValue = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(body["num_anomalies"], 1);
    assert_eq!(body["buckets"][10]["timestamp_millis"], 600_000);
    assert_eq!(body["buckets"][10]["value"], 0.0);
    assert_eq!(body["buckets"][10]["is_anomaly"], true);
    assert!(body["buckets"][10]["z_score"].as_f64().unwrap() < -3.0);
}

#[tokio::test]
async fn test_anomaly_detection_route_invalid_argument() {
    let response = warp::test::request()
        .method("POST")
        .path("/logs/anomalies")
        .json(&json!({ "timestamp_field": "timestamp", "interval": "1m", "alpha": 2.0 }))
        .reply(&anomaly_handler(MockSearchService::new()))
        .await;
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn test_anomaly_detection_route_unknown_field() {
    let response = warp::test::request()
        .method("POST")
        .path("/logs/anomalies")
        .json(&json!({ "timestamp_field": "timestamp", "interval": "1m", "foo": 1 }))
        .reply(&anomaly_handler(MockSearchService::new()))
        .await;
    assert_eq!(response.status(), 400);
}
