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

use std::sync::Arc;

use quickwit_proto::search::{CountHits, SearchRequest, SearchResponse};
use quickwit_query::aggregations::{
    AggregationResult, AggregationResults, BucketEntries, BucketEntry, BucketResult, Key,
    MetricResult,
};
use quickwit_query::query_ast::query_ast_from_user_text;
use quickwit_search::{SearchError, SearchService};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;
use warp::{Filter, Rejection};

use super::detector::{EwmaConfig, score_series};
use crate::rest_api_response::into_rest_api_response;
use crate::search_api::extract_index_id_patterns;
use crate::{BodyFormat, with_arg};

const HISTOGRAM_AGG_NAME: &str = "anomaly_detection_histogram";
const METRIC_AGG_NAME: &str = "anomaly_detection_metric";

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(anomaly_detection_post_handler),
    components(schemas(
        AnomalyDetectionRequest,
        AnomalyDetectionResponse,
        AnomalyBucket,
        AnomalyMetric,
        AnomalyMetricType,
    ))
)]
pub struct AnomalyApi;

fn default_query() -> String {
    "*".to_string()
}

fn default_alpha() -> f64 {
    0.3
}

fn default_threshold() -> f64 {
    3.0
}

fn default_warmup() -> usize {
    5
}

/// Metric computed for each time bucket. When absent, the document count is used.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AnomalyMetricType {
    Avg,
    Sum,
    Min,
    Max,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AnomalyMetric {
    #[serde(rename = "type")]
    pub metric_type: AnomalyMetricType,
    /// Numeric fast field to compute the metric on, e.g. `span_duration_millis`.
    pub field: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AnomalyDetectionRequest {
    /// Query selecting the documents to analyze. Defaults to `*`.
    #[serde(default = "default_query")]
    pub query: String,
    /// Datetime fast field used to build the time series.
    pub timestamp_field: String,
    /// Fixed bucket interval, e.g. `30s`, `1m`, `1h`.
    pub interval: String,
    /// If set, restrict the analysis to documents with `timestamp >= start_timestamp` (seconds).
    #[serde(default)]
    pub start_timestamp: Option<i64>,
    /// If set, restrict the analysis to documents with `timestamp < end_timestamp` (seconds).
    #[serde(default)]
    pub end_timestamp: Option<i64>,
    /// Metric to analyze per bucket. Defaults to the document count.
    #[serde(default)]
    pub metric: Option<AnomalyMetric>,
    /// EWMA smoothing factor in `(0, 1]`.
    #[serde(default = "default_alpha")]
    pub alpha: f64,
    /// Minimum absolute z-score for a bucket to be flagged as anomalous.
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// Number of non-empty buckets used to build the baseline before scoring starts.
    #[serde(default = "default_warmup")]
    pub warmup: usize,
    #[serde(default)]
    pub format: BodyFormat,
}

impl AnomalyDetectionRequest {
    fn ewma_config(&self) -> EwmaConfig {
        EwmaConfig {
            alpha: self.alpha,
            threshold: self.threshold,
            warmup: self.warmup,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, utoipa::ToSchema)]
pub struct AnomalyBucket {
    /// Start of the bucket, in milliseconds since the Unix epoch.
    pub timestamp_millis: i64,
    pub doc_count: u64,
    /// Analyzed value. `null` when the metric is undefined for the bucket (e.g. average over an
    /// empty bucket).
    pub value: Option<f64>,
    /// Baseline mean before this bucket. `null` when the bucket was not scored.
    pub expected: Option<f64>,
    /// Baseline standard deviation before this bucket. `null` when the bucket was not scored.
    pub std_dev: Option<f64>,
    /// `null` when the bucket was not scored (warmup or undefined value).
    pub z_score: Option<f64>,
    pub is_anomaly: bool,
}

#[derive(Debug, PartialEq, Serialize, utoipa::ToSchema)]
pub struct AnomalyDetectionResponse {
    pub num_hits: u64,
    pub num_anomalies: usize,
    pub buckets: Vec<AnomalyBucket>,
    pub elapsed_time_micros: u64,
}

fn build_search_request(
    index_id_patterns: Vec<String>,
    request: &AnomalyDetectionRequest,
) -> Result<SearchRequest, SearchError> {
    request
        .ewma_config()
        .validate()
        .map_err(SearchError::InvalidArgument)?;

    let mut date_histogram = json!({
        "field": request.timestamp_field,
        "fixed_interval": request.interval,
        "min_doc_count": 0,
    });
    // Without bounds, empty buckets are only returned between the first and last non-empty
    // bucket. Forcing the bounds makes silent periods at the edges of the range visible.
    if let (Some(start_timestamp), Some(end_timestamp)) =
        (request.start_timestamp, request.end_timestamp)
    {
        if start_timestamp >= end_timestamp {
            return Err(SearchError::InvalidArgument(
                "`start_timestamp` must be strictly lower than `end_timestamp`".to_string(),
            ));
        }
        let bounds = json!({
            "min": start_timestamp * 1_000,
            // `end_timestamp` is exclusive, bounds are inclusive.
            "max": end_timestamp * 1_000 - 1,
        });
        date_histogram["extended_bounds"] = bounds.clone();
        date_histogram["hard_bounds"] = bounds;
    }
    let mut histogram_agg = json!({ "date_histogram": date_histogram });
    if let Some(metric) = &request.metric {
        let metric_name = match metric.metric_type {
            AnomalyMetricType::Avg => "avg",
            AnomalyMetricType::Sum => "sum",
            AnomalyMetricType::Min => "min",
            AnomalyMetricType::Max => "max",
        };
        histogram_agg["aggs"] = json!({
            METRIC_AGG_NAME: { metric_name: { "field": metric.field } }
        });
    }
    let aggregation_request = json!({ HISTOGRAM_AGG_NAME: histogram_agg });
    let query_ast = query_ast_from_user_text(&request.query, None);

    Ok(SearchRequest {
        index_id_patterns,
        query_ast: serde_json::to_string(&query_ast)?,
        start_timestamp: request.start_timestamp,
        end_timestamp: request.end_timestamp,
        max_hits: 0,
        aggregation_request: Some(aggregation_request.to_string()),
        count_hits: CountHits::CountAll.into(),
        ..Default::default()
    })
}

fn extract_bucket_value(
    bucket: &BucketEntry,
    metric_opt: Option<&AnomalyMetric>,
) -> Result<Option<f64>, SearchError> {
    if metric_opt.is_none() {
        return Ok(Some(bucket.doc_count as f64));
    }
    let metric_result = bucket
        .sub_aggregation
        .0
        .iter()
        .find(|(name, _)| name == METRIC_AGG_NAME)
        .map(|(_, result)| result)
        .ok_or_else(|| {
            SearchError::Internal("missing metric sub-aggregation in histogram bucket".to_string())
        })?;
    let value_opt = match metric_result {
        AggregationResult::MetricResult(
            MetricResult::Average(single)
            | MetricResult::Sum(single)
            | MetricResult::Min(single)
            | MetricResult::Max(single),
        ) => single.value,
        other => {
            return Err(SearchError::Internal(format!(
                "unexpected metric sub-aggregation result: {other:?}"
            )));
        }
    };
    if let Some(value) = value_opt
        && !value.is_finite()
    {
        return Err(SearchError::Internal(format!(
            "metric sub-aggregation returned a non-finite value: {value}"
        )));
    }
    Ok(value_opt)
}

fn histogram_buckets(aggregation_postcard: &[u8]) -> Result<Vec<BucketEntry>, SearchError> {
    let aggregation_results: AggregationResults = postcard::from_bytes(aggregation_postcard)
        .map_err(|error| SearchError::Internal(error.to_string()))?;
    let histogram_result = aggregation_results
        .0
        .into_iter()
        .find(|(name, _)| name == HISTOGRAM_AGG_NAME)
        .map(|(_, result)| result)
        .ok_or_else(|| SearchError::Internal("missing histogram aggregation".to_string()))?;
    let AggregationResult::BucketResult(BucketResult::Histogram {
        buckets: BucketEntries::Vec(buckets),
    }) = histogram_result
    else {
        return Err(SearchError::Internal(format!(
            "unexpected histogram aggregation result: {histogram_result:?}"
        )));
    };
    Ok(buckets)
}

fn build_response(
    search_response: SearchResponse,
    request: &AnomalyDetectionRequest,
) -> Result<AnomalyDetectionResponse, SearchError> {
    if let Some(search_error) = SearchError::from_split_errors(&search_response.failed_splits) {
        // A partial result would make missing data look like a drop.
        return Err(search_error);
    }
    // The root search returns no aggregation at all when no index matched any split.
    let buckets = match &search_response.aggregation_postcard {
        Some(aggregation_postcard) => histogram_buckets(aggregation_postcard)?,
        None => Vec::new(),
    };
    let mut timestamps_millis = Vec::with_capacity(buckets.len());
    let mut doc_counts = Vec::with_capacity(buckets.len());
    let mut values = Vec::with_capacity(buckets.len());
    for bucket in &buckets {
        let Key::F64(timestamp_millis) = bucket.key else {
            return Err(SearchError::Internal(format!(
                "unexpected date histogram key: {:?}",
                bucket.key
            )));
        };
        timestamps_millis.push(timestamp_millis as i64);
        doc_counts.push(bucket.doc_count);
        values.push(extract_bucket_value(bucket, request.metric.as_ref())?);
    }
    let scores = score_series(&values, &request.ewma_config());
    let anomaly_buckets: Vec<AnomalyBucket> = timestamps_millis
        .into_iter()
        .zip(doc_counts)
        .zip(values)
        .zip(scores)
        .map(
            |(((timestamp_millis, doc_count), value), score_opt)| AnomalyBucket {
                timestamp_millis,
                doc_count,
                value,
                expected: score_opt.map(|score| score.expected),
                std_dev: score_opt.map(|score| score.std_dev),
                z_score: score_opt.map(|score| score.z_score),
                is_anomaly: matches!(score_opt, Some(score) if score.is_anomaly),
            },
        )
        .collect();
    let num_anomalies = anomaly_buckets
        .iter()
        .filter(|bucket| bucket.is_anomaly)
        .count();
    Ok(AnomalyDetectionResponse {
        num_hits: search_response.num_hits,
        num_anomalies,
        buckets: anomaly_buckets,
        elapsed_time_micros: search_response.elapsed_time_micros,
    })
}

async fn anomaly_detection_endpoint(
    index_id_patterns: Vec<String>,
    request: &AnomalyDetectionRequest,
    search_service: &dyn SearchService,
) -> Result<AnomalyDetectionResponse, SearchError> {
    let search_request = build_search_request(index_id_patterns, request)?;
    let search_response = search_service.root_search(search_request).await?;
    build_response(search_response, request)
}

async fn anomaly_detection(
    index_id_patterns: Vec<String>,
    request: AnomalyDetectionRequest,
    search_service: Arc<dyn SearchService>,
) -> impl warp::Reply {
    info!(request =? request, "anomaly-detection");
    let body_format = request.format;
    let result = anomaly_detection_endpoint(index_id_patterns, &request, &*search_service).await;
    into_rest_api_response(result, body_format)
}

fn anomaly_detection_post_filter()
-> impl Filter<Extract = (Vec<String>, AnomalyDetectionRequest), Error = Rejection> + Clone {
    warp::path!(String / "anomalies")
        .and_then(extract_index_id_patterns)
        .and(warp::post())
        .and(warp::body::content_length_limit(1024 * 1024))
        .and(warp::body::json())
}

#[utoipa::path(
    post,
    tag = "Search",
    path = "/{index_id}/anomalies",
    request_body = AnomalyDetectionRequest,
    responses(
        (status = 200, description = "Successfully scored the time series.", body = AnomalyDetectionResponse)
    ),
    params(
        ("index_id" = String, Path, description = "The index ID(s) to analyze."),
    )
)]
/// Detect Anomalies
///
/// Buckets the matching documents over time, computes a metric per bucket (document count by
/// default) and flags the buckets deviating from an exponentially weighted moving baseline.
pub fn anomaly_detection_post_handler(
    search_service: Arc<dyn SearchService>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = Rejection> + Clone {
    anomaly_detection_post_filter()
        .and(with_arg(search_service))
        .then(anomaly_detection)
}

#[cfg(test)]
#[path = "rest_handler_tests.rs"]
mod tests;
