use openmetrics_parser::{Exemplar, MetricNumber, PrometheusCounterValue, PrometheusValue, Sample};

use crate::aggregator::*;
use std::{collections::HashMap, str::FromStr};

#[test]
fn test_clear_mode_parsing() {
    assert!(ClearMode::from_str("replace").is_ok());
    assert_eq!(ClearMode::from_str("replace").unwrap(), ClearMode::Replace);

    assert!(ClearMode::from_str("aggregate").is_ok());
    assert_eq!(ClearMode::from_str("aggregate").unwrap(), ClearMode::Aggregate);

    assert!(ClearMode::from_str("family").is_ok());
    assert_eq!(ClearMode::from_str("family").unwrap(), ClearMode::Family);

    assert!(ClearMode::from_str("foo").is_err());
}

#[test]
fn test_clear_mode_replace() {
    let mut sample = Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Gauge(MetricNumber::Int(1))));
    merge_metric(&mut sample, 
                Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Gauge(MetricNumber::Int(2)))),
                      ClearMode::Replace).unwrap();

    assert_eq!(sample.value, GravelValue::Prometheus(PrometheusValue::Gauge(MetricNumber::Int(2))));

    // Test that exemplars get replaced
    let mut sample = Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Counter(PrometheusCounterValue{
        value: MetricNumber::Int(1),
        exemplar: None,
    })));

    merge_metric(&mut sample, 
                Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Counter(
                    PrometheusCounterValue{
                        value: MetricNumber::Int(1000),
                        exemplar: Some(Exemplar {
                            labels: HashMap::new(),
                            timestamp: None,
                            id: 1000.,
                        }),
                    }
                ))),
                ClearMode::Replace).unwrap();

    assert_eq!(sample.value, GravelValue::Prometheus(PrometheusValue::Counter(PrometheusCounterValue{
        value: MetricNumber::Int(1000),
        exemplar: Some(Exemplar {
            labels: HashMap::new(),
            timestamp: None,
            id: 1000.,
        }),
    })));

    let mut sample = Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Counter(PrometheusCounterValue{
        value: MetricNumber::Int(1),
        exemplar: Some(Exemplar {
            labels: HashMap::new(),
            timestamp: None,
            id: 1000.,
        }),
    })));

    merge_metric(&mut sample, 
                Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Counter(
                    PrometheusCounterValue{
                        value: MetricNumber::Int(1000),
                        exemplar: None
                    }
                ))),
                ClearMode::Replace).unwrap();

    assert_eq!(sample.value, GravelValue::Prometheus(PrometheusValue::Counter(PrometheusCounterValue{
        value: MetricNumber::Int(1000),
        exemplar: None,
    })));
}

#[test]
fn test_clear_mode_aggregate() {
    let mut sample = Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Gauge(MetricNumber::Int(1))));
    merge_metric(&mut sample, 
                Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Gauge(MetricNumber::Int(2)))),
                      ClearMode::Aggregate).unwrap();

    assert_eq!(sample.value, GravelValue::Prometheus(PrometheusValue::Gauge(MetricNumber::Int(3))));

    // Test that exemplars get replaced
    let mut sample = Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Counter(PrometheusCounterValue{
        value: MetricNumber::Int(1),
        exemplar: None,
    })));

    merge_metric(&mut sample, 
                Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Counter(
                    PrometheusCounterValue{
                        value: MetricNumber::Int(1000),
                        exemplar: Some(Exemplar {
                            labels: HashMap::new(),
                            timestamp: None,
                            id: 1000.,
                        }),
                    }
                ))),
                ClearMode::Aggregate).unwrap();

    assert_eq!(sample.value, GravelValue::Prometheus(PrometheusValue::Counter(PrometheusCounterValue{
        value: MetricNumber::Int(1001),
        exemplar: Some(Exemplar {
            labels: HashMap::new(),
            timestamp: None,
            id: 1000.,
        }),
    })));

    let mut sample = Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Counter(PrometheusCounterValue{
        value: MetricNumber::Int(1),
        exemplar: Some(Exemplar {
            labels: HashMap::new(),
            timestamp: None,
            id: 1000.,
        }),
    })));

    merge_metric(&mut sample, 
                Sample::new(vec![], None, GravelValue::Prometheus(PrometheusValue::Counter(
                    PrometheusCounterValue{
                        value: MetricNumber::Int(1000),
                        exemplar: None
                    }
                ))),
                ClearMode::Aggregate).unwrap();

    assert_eq!(sample.value, GravelValue::Prometheus(PrometheusValue::Counter(PrometheusCounterValue{
        value: MetricNumber::Int(1001),
        exemplar: None,
    })));
}

#[tokio::test]
async fn test_push_with_different_label_names() {
    let mut agg = Aggregator::new();
    assert!(agg.parse_and_merge("requests_num_total{LAMBDA_NAME=\"test_function\"} 1\n", &HashMap::new()).await.is_ok(), "failed to parse valid metric");
    assert!(agg.parse_and_merge("requests_num_total{job=\"test\"} 1\n", &HashMap::new()).await.is_err(), "failed to reject invalid label name");
    assert!(agg.parse_and_merge("requests_num_total{bar=\"test\"} 1\n", &HashMap::new()).await.is_err(), "failed to reject invalid label name");
    assert!(agg.parse_and_merge("requests_num_total{LAMBDA_NAME=\"test_function\"} 1\n", &HashMap::new()).await.is_ok(), "failed to parse metric with same label name");

    assert!(agg.parse_and_merge("requests_num_total2{clearmode=\"mean5m\"} 1\n", &HashMap::new()).await.is_ok(), "failed to add metric with clearmode");
    assert!(agg.parse_and_merge("requests_num_total2{clearmode=\"mean5m\"} 1\n", &HashMap::new()).await.is_ok(), "failed to add second metric with clearmode");
}
