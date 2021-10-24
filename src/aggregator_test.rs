use openmetrics_parser::{MetricNumber, PrometheusValue, Sample};

use crate::aggregator::*;
use std::str::FromStr;

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

fn test_clear_mode_replace() {
    let mut sample = Sample::new(vec![], None, PrometheusValue::Gauge(MetricNumber::Int(1)));
    merge_metric(&mut sample, 
                Sample::new(vec![], None, PrometheusValue::Gauge(MetricNumber::Int(2))),
                      ClearMode::Replace);

    assert_eq!(sample.value, PrometheusValue::Gauge(MetricNumber::Int(2)));
}