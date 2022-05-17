use std::{collections::HashMap, str::FromStr, sync::Arc, fmt, time::Duration};

use openmetrics_parser::{RenderableMetricValue, HistogramBucket, MetricsExposition, ParseError, PrometheusMetricFamily, PrometheusType, PrometheusValue, Sample, prometheus, MetricFamily, Timestamp, MetricNumber};
use tokio::sync::RwLock;

use crate::pebble::{TimePebble, parse_duration, sum_merge_strategy, mean_merge_strategy};

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

#[derive(Debug, Clone, PartialEq)]
pub enum ClearMode {
    Aggregate,
    Replace,
    Family,
    Mean(Duration),
    Sum(Duration)
}

type GravelMetricFamily = MetricFamily<PrometheusType, GravelValue>;

#[derive(Debug, Clone, PartialEq)]
pub enum GravelValue {
    Prometheus(PrometheusValue),
    Pebble(TimePebble)
}

impl RenderableMetricValue for GravelValue {
    fn render(
        &self,
        f: &mut fmt::Formatter<'_>,
        metric_name: &str,
        timestamp: Option<&Timestamp>,
        label_names: &[&str],
        label_values: &[&str],
    ) -> fmt::Result {
        match self {
            GravelValue::Prometheus(v) => v.render(f, metric_name, timestamp, label_names, label_values),
            GravelValue::Pebble(pebble) => {
                let value = pebble.aggregate();

                return PrometheusValue::Gauge(MetricNumber::Float(value)).render(f, metric_name, timestamp, label_names, label_values);
            }
        }
    }
}

impl From<PrometheusValue> for GravelValue {
    fn from(prom: PrometheusValue) -> Self {
        return GravelValue::Prometheus(prom.clone());
    }
}

impl GravelValue {
    fn convert_with_clearmode(self, clearmode: ClearMode) -> GravelValue {
        const DEFAULT_PEBBLE_GRANULARITY: usize = 100;
        match clearmode {
            ClearMode::Sum(duration) => {
                return GravelValue::Pebble(TimePebble::new(duration, DEFAULT_PEBBLE_GRANULARITY, sum_merge_strategy));
            },
            ClearMode::Mean(duration) => {
                return GravelValue::Pebble(TimePebble::new(duration, DEFAULT_PEBBLE_GRANULARITY, mean_merge_strategy));
            }
            _ => return self
        }
    }
}

impl ClearMode {
    fn default_for_type(t: PrometheusType) -> ClearMode {
        match t {
            PrometheusType::Counter | PrometheusType::Unknown | PrometheusType::Histogram | PrometheusType::Summary => ClearMode::Aggregate,
            PrometheusType::Gauge => ClearMode::Replace,
        }
    }

    fn from_family<T>(family_type: PrometheusType, metric: &Sample<T>) -> ClearMode where T: RenderableMetricValue + Clone {
        match metric.get_labelset().unwrap().get_label_value(CLEARMODE_LABEL_NAME) {
            Some(c) => ClearMode::from_str(c).unwrap_or(ClearMode::default_for_type(family_type)),
            None => ClearMode::default_for_type(family_type)
        }
    }
}

impl FromStr for ClearMode {
    type Err = AggregationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "aggregate" | "sum" => Ok(ClearMode::Aggregate),
            "replace" => Ok(ClearMode::Replace),
            "family" | "info" => Ok(ClearMode::Family),
            _ => {
                if s.starts_with("mean") || s.starts_with("sum") {
                    let num_preceeding = s.chars().take_while(|c| c.is_digit(10)).count();
                    match parse_duration(&s[num_preceeding..]) {
                        Some(duration) => {
                            if s.starts_with("mean") {
                                return Ok(ClearMode::Mean(duration))
                            }

                            if s.starts_with("sum") {
                                return Ok(ClearMode::Sum(duration))
                            }
                        },
                        None => return Err(AggregationError::Error(format!("Invalid duration string: {}", &s[num_preceeding..])))
                    }
                }

                Err(AggregationError::Error(format!("Invalid clearmode: {}", s)))
            }
        }
    }
}

/// An aggregation family is a wrapped around a normal metrics family that is able to aggregate
/// new families into itself
#[derive(Debug)]
struct AggregationFamily {
    base_family: GravelMetricFamily,
}

/// Takes two sets of Histogram buckets and merges them. Assumes that they are in ascending order of upperbound
/// (TODO: We should probably sanity check this / sort) and performs essentially a merge sort merge, summing the counts
/// if two buckets have the same bound
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

/// Merges two metrics into one another (using the given clearmode), storing the result in the first one.
pub fn merge_metric(into: &mut Sample<GravelValue>, merge: Sample<GravelValue>, clear_mode: ClearMode) {
    match (&mut into.value, &merge.value) {
        (GravelValue::Prometheus(PrometheusValue::Unknown(val1)), GravelValue::Prometheus(PrometheusValue::Unknown(val2))) => {
            match clear_mode {
                ClearMode::Aggregate => *val1 += val2,
                ClearMode::Replace => *val1 = *val2,
                _ => unreachable!()
            }
        }
        (GravelValue::Prometheus(PrometheusValue::Gauge(val1)), GravelValue::Prometheus(PrometheusValue::Gauge(val2))) => {
            match clear_mode {
                ClearMode::Aggregate => *val1 += val2,
                ClearMode::Replace => *val1 = *val2,
                _ => unreachable!()
            }
        }
        (GravelValue::Prometheus(PrometheusValue::Counter(val1)), GravelValue::Prometheus(PrometheusValue::Counter(val2))) => {
            // Counters get a bit more complicated - we take the second exemplar no matter what
            match clear_mode {
                ClearMode::Aggregate => {
                    val1.value += val2.value;
                    val1.exemplar = val2.exemplar.clone();
                }
                ClearMode::Replace => {
                    val1.value = val2.value;
                    val1.exemplar = val2.exemplar.clone();
                },
                _ => unreachable!()
            }
        }
        (GravelValue::Prometheus(PrometheusValue::Histogram(val1)), GravelValue::Prometheus(PrometheusValue::Histogram(val2))) => {
            let sum = match (val1.sum, val2.sum, &clear_mode) {
                (Some(a), Some(b), ClearMode::Aggregate) => Some(a + b),
                (Some(_), Some(b), ClearMode::Replace) => Some(b),
                _ => None,
            };

            let count = match (val1.count, val2.count, &clear_mode) {
                (Some(a), Some(b), ClearMode::Aggregate) => Some(a + b),
                (Some(_), Some(b), ClearMode::Replace) => Some(b),
                _ => None,
            };

            let buckets = match clear_mode {
                ClearMode::Aggregate => merge_buckets(&val1.buckets, &val2.buckets),
                ClearMode::Replace => val2.buckets.clone(),
                _ => unreachable!()
            };

            val1.sum = sum;
            val1.count = count;
            val1.buckets = buckets;
            val1.created = val2.created;
        }
        (GravelValue::Pebble(time_pebble), GravelValue::Prometheus(p)) => {
            match p {
                PrometheusValue::Counter(counter) => time_pebble.append(counter.value.as_f64()),
                PrometheusValue::Gauge(gauge) => time_pebble.append(gauge.as_f64()),
                _ => {}
            }
        },
        (GravelValue::Prometheus(PrometheusValue::Summary(_)), GravelValue::Prometheus(PrometheusValue::Summary(_))) => todo!(),
        _ => unreachable!(),
    };
}

impl AggregationFamily {
    // Constructs a new AggregationFamily, over the given MetricFamily
    fn new(base_family: PrometheusMetricFamily) -> Self {
        let mut base_family: GravelMetricFamily = base_family.clone_and_convert_type();
        let family_type = base_family.family_type.clone();
        for metric in base_family.iter_samples_mut() {
            let clear_mode = ClearMode::from_family(family_type.clone(), &metric);
            metric.value = metric.value.clone().convert_with_clearmode(clear_mode);
        }

        let base_family = base_family.without_label(CLEARMODE_LABEL_NAME).unwrap_or(base_family);
        Self { base_family }
    }

    /// Merges the given metrics family into this one, respecting (and then removing) the clear mode 
    /// label from each sample
    fn merge(&mut self, prom_family: PrometheusMetricFamily) -> Result<(), AggregationError> {
        let new_family = prom_family.clone_and_convert_type();
        // Sanity checks to make sure that it makes sense to merge these families
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

        // We should clear the whole family if any of the samples has a clearmode="family" label
        let should_clear_family = new_family.iter_samples().any(|metric| {
            ClearMode::from_family(new_family.family_type.clone(), metric) == ClearMode::Family
        });

        if should_clear_family {
            self.base_family = new_family.without_label(CLEARMODE_LABEL_NAME).unwrap_or(new_family);
        }
        else {
            for metric in new_family.into_iter_samples() {
                // TODO: This is really inefficient for large families. Should probably optimise it
                // Go uses "label fingerprinting" to generate hashes of labelsets.

                // We want to compare without the clearmode label - it's not stored, so doesn't exist in our internal representation
                let cmp_metric = metric.without_label(CLEARMODE_LABEL_NAME).unwrap_or(metric.clone());
                let clear_mode = ClearMode::from_family(self.base_family.family_type.clone(), &metric);
                match self.base_family.get_sample_matches_mut(&cmp_metric)
                {
                    None => {
                        // Just add the metric if its a new labelset
                        self.base_family.add_sample(cmp_metric)?
                    },
                    Some(s) => {
                        // Otherwise we have to merge
                        merge_metric(s, metric, clear_mode);
                    }
                }
            }
        }
        
        return Ok(());
    }
}

/// Aggregator is an struct that stores a number of metric families, and has the ability to merge
/// new metric families into itself
#[derive(Debug, Clone)]
pub struct Aggregator {
    /// The families in this Aggregator
    families: Arc<RwLock<HashMap<String, AggregationFamily>>>,
}

/// A utility function that adds a set of labels to all the metrics in an exposition
/// This is used to handle the push gateway /metrics/job/foo URL syntax to add a job=foo label
fn add_extra_labels(mut exposition: MetricsExposition<PrometheusType, PrometheusValue>, extra_labels: &HashMap<&str, &str>) -> Result<MetricsExposition<PrometheusType, PrometheusValue>, ParseError> {
    exposition.families = exposition.families.into_iter().map(|(name, family)| (name, family.with_labels(extra_labels.iter().map(|(&k, &v)| (k, v))))).collect();

    return Ok(exposition);
}

impl Aggregator {
    pub fn new() -> Aggregator {
        return Aggregator {
            families: Arc::new(RwLock::new(HashMap::new())),
        };
    }

    /// Takes a string representing a Prometheus exposition format, parses that and 
    /// merges the metrics into this aggregator
    pub async fn parse_and_merge(&mut self, s: &str, extra_labels: &HashMap<&str, &str>) -> Result<(), AggregationError> {
        let metrics = add_extra_labels(prometheus::parse_prometheus(&s)?, extra_labels)?;
        let mut families = self.families.write().await;

        for (name, metrics) in metrics.families {
            match families.get_mut(&name) {
                Some(f) => {
                    // If we have the family already, merge this new stuff into it
                    f.merge(metrics)?;
                }
                None => {
                    // Otherwise, just add the new family
                    families.insert(name, AggregationFamily::new(metrics));
                }
            }
        }

        return Ok(());
    }

    /// Converts this aggregator into a Prometheus text exposition format
    /// that can be scraped by a Prometheus
    pub async fn to_string(&self) -> String {
        let families = self.families.read().await;
        let mut family_strings = String::new();
        for (_, family) in families.iter() {
            family_strings.push_str(&family.base_family.to_string());
        }

        family_strings
    }
}
