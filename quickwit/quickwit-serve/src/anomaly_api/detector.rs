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

//! Streaming anomaly detector based on an exponentially weighted moving average (EWMA)
//! and an exponentially weighted moving variance.
//!
//! Each point is scored against the baseline built from the points *before* it, and only then
//! folded into the baseline. The z-score of a point is `(value - mean) / std_dev`.

/// Lower bound applied to the standard deviation when computing z-scores.
///
/// A perfectly flat baseline has a variance of zero. Without a floor, any deviation from it
/// would yield an infinite z-score, which cannot be serialized to JSON. With the floor, such a
/// deviation yields a very large but finite z-score and is still flagged.
const MIN_STD_DEV: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EwmaConfig {
    /// Smoothing factor in `(0, 1]`. Higher values make the baseline react faster.
    pub alpha: f64,
    /// A point is anomalous when `|z_score| >= threshold`. Must be strictly positive.
    pub threshold: f64,
    /// Number of observed points required before any point gets scored. Must be at least 1.
    pub warmup: usize,
}

impl EwmaConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.alpha > 0.0 && self.alpha <= 1.0) {
            return Err(format!(
                "`alpha` must be in the range (0, 1], got `{}`",
                self.alpha
            ));
        }
        if !(self.threshold.is_finite() && self.threshold > 0.0) {
            return Err(format!(
                "`threshold` must be a strictly positive number, got `{}`",
                self.threshold
            ));
        }
        if self.warmup == 0 {
            return Err("`warmup` must be at least 1".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnomalyScore {
    /// Baseline mean before observing the point.
    pub expected: f64,
    /// Baseline standard deviation before observing the point.
    pub std_dev: f64,
    pub z_score: f64,
    pub is_anomaly: bool,
}

/// Scores a series of values, in order.
///
/// `None` values (e.g. an average over an empty bucket) are left unscored and do not update
/// the baseline. Points observed during the warmup phase are also left unscored.
///
/// Precondition: `config` is valid (see [`EwmaConfig::validate`]) and all values are finite.
pub fn score_series(values: &[Option<f64>], config: &EwmaConfig) -> Vec<Option<AnomalyScore>> {
    let mut mean: f64 = 0.0;
    let mut variance: f64 = 0.0;
    let mut num_observed: usize = 0;
    let mut scores = Vec::with_capacity(values.len());

    for value_opt in values {
        let Some(value) = *value_opt else {
            scores.push(None);
            continue;
        };
        if num_observed == 0 {
            mean = value;
            num_observed = 1;
            scores.push(None);
            continue;
        }
        let deviation = value - mean;
        let score_opt = if num_observed >= config.warmup {
            let std_dev = variance.sqrt();
            let z_score = deviation / std_dev.max(MIN_STD_DEV);
            Some(AnomalyScore {
                expected: mean,
                std_dev,
                z_score,
                is_anomaly: z_score.abs() >= config.threshold,
            })
        } else {
            None
        };
        scores.push(score_opt);

        // Incremental EWMA update of mean and variance (Finch, 2009).
        let increment = config.alpha * deviation;
        mean += increment;
        variance = (1.0 - config.alpha) * (variance + deviation * increment);
        num_observed += 1;
    }
    scores
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(alpha: f64, threshold: f64, warmup: usize) -> EwmaConfig {
        EwmaConfig {
            alpha,
            threshold,
            warmup,
        }
    }

    #[test]
    fn test_ewma_config_validate() {
        assert!(config(0.3, 3.0, 5).validate().is_ok());
        assert!(config(1.0, 0.5, 1).validate().is_ok());
        assert!(config(0.0, 3.0, 5).validate().is_err());
        assert!(config(1.1, 3.0, 5).validate().is_err());
        assert!(config(f64::NAN, 3.0, 5).validate().is_err());
        assert!(config(0.3, 0.0, 5).validate().is_err());
        assert!(config(0.3, f64::INFINITY, 5).validate().is_err());
        assert!(config(0.3, 3.0, 0).validate().is_err());
    }

    #[test]
    fn test_score_series_empty() {
        assert!(score_series(&[], &config(0.3, 3.0, 1)).is_empty());
    }

    #[test]
    fn test_score_series_detects_spike() {
        let mut values: Vec<Option<f64>> = (0..20)
            .map(|i| Some(if i % 2 == 0 { 100.0 } else { 104.0 }))
            .collect();
        values.push(Some(500.0));
        values.push(Some(102.0));
        let scores = score_series(&values, &config(0.3, 3.0, 5));
        assert_eq!(scores.len(), values.len());
        for score_opt in &scores[..5] {
            assert!(score_opt.is_none());
        }
        for score_opt in &scores[5..20] {
            assert!(!score_opt.unwrap().is_anomaly);
        }
        let spike_score = scores[20].unwrap();
        assert!(spike_score.is_anomaly);
        assert!(spike_score.z_score > 3.0);
        assert!((spike_score.expected - 102.0).abs() < 3.0);
    }

    #[test]
    fn test_score_series_detects_drop() {
        let mut values: Vec<Option<f64>> = (0..20)
            .map(|i| Some(if i % 2 == 0 { 50.0 } else { 55.0 }))
            .collect();
        values.push(Some(0.0));
        let scores = score_series(&values, &config(0.3, 3.0, 5));
        let drop_score = scores[20].unwrap();
        assert!(drop_score.is_anomaly);
        assert!(drop_score.z_score < -3.0);
    }

    #[test]
    fn test_score_series_flat_baseline() {
        let values = [Some(10.0), Some(10.0), Some(10.0), Some(10.0), Some(11.0)];
        let scores = score_series(&values, &config(0.3, 3.0, 2));
        let flat_score = scores[3].unwrap();
        assert_eq!(flat_score.z_score, 0.0);
        assert!(!flat_score.is_anomaly);
        let deviating_score = scores[4].unwrap();
        assert_eq!(deviating_score.std_dev, 0.0);
        assert!(deviating_score.z_score.is_finite());
        assert!(deviating_score.is_anomaly);
    }

    #[test]
    fn test_score_series_skips_missing_values() {
        let values = [Some(1.0), None, Some(1.0), None, Some(1.0)];
        let scores = score_series(&values, &config(0.5, 3.0, 2));
        assert!(scores[0].is_none());
        assert!(scores[1].is_none());
        // Only one point observed so far: still in warmup.
        assert!(scores[2].is_none());
        assert!(scores[3].is_none());
        assert_eq!(scores[4].unwrap().z_score, 0.0);
    }

    #[test]
    fn test_score_series_matches_manual_computation() {
        let values = [Some(2.0), Some(4.0), Some(8.0)];
        let scores = score_series(&values, &config(0.5, 10.0, 1));
        // After 2.0: mean = 2, var = 0.
        let second = scores[1].unwrap();
        assert_eq!(second.expected, 2.0);
        assert_eq!(second.std_dev, 0.0);
        // After 4.0: deviation = 2, increment = 1, mean = 3, var = 0.5 * (0 + 2 * 1) = 1.
        let third = scores[2].unwrap();
        assert_eq!(third.expected, 3.0);
        assert_eq!(third.std_dev, 1.0);
        assert_eq!(third.z_score, 5.0);
        assert!(!third.is_anomaly);
    }
}
