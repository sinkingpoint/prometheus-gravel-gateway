use std::{collections::HashMap, str::FromStr, sync::Arc};

use openmetrics_parser::{HistogramBucket, HistogramValue, MetricsExposition, ParseError, PrometheusCounterValue, PrometheusMetricFamily, PrometheusValue, Sample, prometheus};
use tokio::sync::RwLock;

const CLEARMODE_LABEL_NAME: &str = "clearmode";

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

#[derive(Debug, Clone)]
enum ClearMode {
    Aggregate,
    Replace,
    Family,
    NonExistent,
    OnScrape
}

impl FromStr for ClearMode {
    type Err = AggregationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "aggregate" => Ok(ClearMode::Aggregate),
            "replace" => Ok(ClearMode::Replace),
            "family" => Ok(ClearMode::Family),
            "onscrape" => Ok(ClearMode::OnScrape),
            "nonexistent" => Ok(ClearMode::NonExistent),
            _ => Err(AggregationError::Error(format!("Invalid clearmode: {}", s)))
        }
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
                created: val2.created,
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
        if new_family.family_name != self.base_family.family_name {
            return Err(AggregationError::Error(format!(
                "Invalid metric names - tried to merge {} with {}",
                new_family.family_name, self.base_family.family_name
            )));
        }

        if new_family.family_type != self.base_family.family_type {
            return Err(AggregationError::Error(format!(
                "Invalid metric types - tried to merge {:?} with {:?}",
                new_family.family_type, self.base_family.family_type
            )));
        }

        for metric in new_family.into_iter_samples() {
            // TODO: This is really inefficient for large families. Should probably optimise it
            match self.base_family.get_sample_matches_mut(&metric)
            {
                None => self.base_family.add_sample(metric).unwrap(),
                Some(s) => {
                    merge_metric(s, metric);
                }
            }
        }

        return Ok(());
    }
}

#[derive(Debug, Clone)]
pub struct Aggregator {
    families: Arc<RwLock<HashMap<String, AggregationFamily>>>,
}

fn add_extra_labels<T, V>(mut exposition: MetricsExposition<T, V>, extra_labels: &HashMap<&str, &str>) -> MetricsExposition<T, V> {
    for (_, metrics) in exposition.families.iter_mut() {
        for (&label_name, &label_value) in extra_labels.iter() {
            metrics.set_label(label_name, label_value);
        }
    }

    return exposition;
}

impl Aggregator {
    pub fn new() -> Aggregator {
        return Aggregator {
            families: Arc::new(RwLock::new(HashMap::new())),
        };
    }

    pub async fn parse_and_merge(&mut self, s: &str, extra_labels: &HashMap<&str, &str>) -> Result<(), AggregationError> {
        let metrics = add_extra_labels(prometheus::parse_prometheus(&s)?, extra_labels);
        let mut families = self.families.write().await;

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

    pub async fn to_string(&self) -> String {
        let families = self.families.read().await;
        let mut base = String::new();
        for (_, family) in families.iter() {
            base.push_str(&format!("{}", family.base_family));
        }

        base
    }
}
