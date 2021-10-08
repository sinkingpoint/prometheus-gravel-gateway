use std::collections::HashMap;

use openmetrics_parser::{
    prometheus, HistogramBucket, HistogramValue, ParseError, PrometheusCounterValue,
    PrometheusMetricFamily, PrometheusValue, Sample,
};
use tokio::sync::Mutex;

#[derive(Debug)]
pub enum AggregationError {
    ParseError(ParseError),
    Error(String),
}

impl From<ParseError> for AggregationError {
    fn from(e: ParseError) -> Self {
        return AggregationError::ParseError(e);
    }
}

#[derive(Debug)]
struct AggregationFamily {
    base_family: PrometheusMetricFamily,
}

fn merge_buckets(val1: &Vec<HistogramBucket>, val2: &Vec<HistogramBucket>) -> Vec<HistogramBucket> {
    let mut i = 0;
    let mut j = 0;
    let mut output = Vec::new();

    // Basically merge sort on the buckets with a bit of extra logic for buckets that have the same upperbound
    while i < val1.len() && j < val2.len() {
        let bucket1 = &val1[i];
        let bucket2 = &val2[j];
        if bucket1.upper_bound < bucket2.upper_bound {
            output.push(bucket1.clone());
            i += 1;
        } else if bucket1.upper_bound > bucket2.upper_bound {
            output.push(bucket2.clone());
            j += 1;
        } else {
            output.push(HistogramBucket {
                count: bucket1.count + bucket2.count,
                upper_bound: bucket1.upper_bound,
                exemplar: bucket2.exemplar.clone(),
            });
            i += 1;
            j += 1;
        }
    }

    for i in i..val1.len() {
        output.push(val1[i].clone());
    }

    for j in j..val2.len() {
        output.push(val1[j].clone());
    }

    return output;
}

fn merge_metric(into: &mut Sample<PrometheusValue>, merge: Sample<PrometheusValue>) {
    into.value = match (&into.value, &merge.value) {
        (PrometheusValue::Unknown(val1), PrometheusValue::Unknown(val2)) => {
            PrometheusValue::Unknown(val1 + val2)
        }
        (PrometheusValue::Gauge(_), PrometheusValue::Gauge(val2)) => {
            PrometheusValue::Gauge(val2.clone())
        }
        (PrometheusValue::Counter(val1), PrometheusValue::Counter(val2)) => {
            PrometheusValue::Counter(PrometheusCounterValue {
                value: val1.value + val2.value,
                exemplar: val2.exemplar.clone(),
            })
        }
        (PrometheusValue::Histogram(val1), PrometheusValue::Histogram(val2)) => {
            let sum = match (val1.sum, val2.sum) {
                (None, None) => None,
                (None, Some(a)) | (Some(a), None) => Some(a),
                (Some(a), Some(b)) => Some(&a + &b),
            };

            let count = match (val1.count, val2.count) {
                (None, None) => None,
                (None, Some(a)) | (Some(a), None) => Some(a),
                (Some(a), Some(b)) => Some(a + b),
            };

            PrometheusValue::Histogram(HistogramValue {
                sum,
                count,
                timestamp: val2.timestamp,
                buckets: merge_buckets(&val1.buckets, &val2.buckets),
            })
        }
        (PrometheusValue::Summary(_), PrometheusValue::Summary(_)) => todo!(),
        _ => unreachable!(),
    }
}

impl AggregationFamily {
    fn new(base_family: PrometheusMetricFamily) -> Self {
        Self { base_family }
    }

    fn merge(&mut self, new_family: PrometheusMetricFamily) -> Result<(), AggregationError> {
        if new_family.name != self.base_family.name {
            return Err(AggregationError::Error(format!(
                "Invalid metric names - tried to merge {} with {}",
                new_family.name, self.base_family.name
            )));
        }

        if new_family.family_type != self.base_family.family_type {
            return Err(AggregationError::Error(format!(
                "Invalid metric types - tried to merge {:?} with {:?}",
                new_family.family_type, self.base_family.family_type
            )));
        }

        if new_family.label_names != self.base_family.label_names {}

        for metric in new_family.metrics {
            // TODO: This is really inefficient for large families. Should probably optimise it
            match self
                .base_family
                .metrics
                .iter_mut()
                .find(|m| m.label_values == metric.label_values)
            {
                None => self.base_family.metrics.push(metric),
                Some(s) => {
                    merge_metric(s, metric);
                }
            }
        }

        return Ok(());
    }
}

#[derive(Debug)]
pub struct Aggregator {
    families: Mutex<HashMap<String, AggregationFamily>>,
}

impl Aggregator {
    pub fn new() -> Aggregator {
        return Aggregator {
            families: Mutex::new(HashMap::new()),
        };
    }

    pub async fn parse_and_merge(&mut self, s: &str) -> Result<(), AggregationError> {
        let metrics = prometheus::parse_prometheus(&s)?;
        let mut families = self.families.lock().await;

        for (name, metrics) in metrics.families {
            match families.get_mut(&name) {
                Some(f) => {
                    f.merge(metrics)?;
                }
                None => {
                    families.insert(name, AggregationFamily::new(metrics));
                }
            }
        }

        return Ok(());
    }
}
