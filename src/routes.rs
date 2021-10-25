use std::{collections::HashMap, sync::Arc};

use warp::{Filter, http::HeaderValue, hyper::{HeaderMap, body::Bytes}, path::Tail, reject::Reject};

use crate::{aggregator::{AggregationError, Aggregator}, auth::Authenticator};

#[derive(Debug)]
enum GravelError {
    Error(String),
    AuthError,
    AggregationError(AggregationError)
}

impl Reject for GravelError {}

pub struct RoutesConfig {
    pub authenticator: Box<dyn Authenticator + Send + Sync>
}

async fn auth(config: Arc<RoutesConfig>, header: String) -> Result<(), warp::Rejection> {
    if config.authenticator.authenticate(&header) {
        return Ok(());
    }

    return Err(warp::reject::custom(GravelError::AuthError));
}

pub fn get_routes(aggregator: Aggregator, config: RoutesConfig) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let config = Arc::new(config);
    let push_metrics_path = warp::path("metrics")
        .and(warp::post())
        .and(warp::header::<String>("authorization").and_then(move |header| auth(config.clone(), header)).untuple_one())
        .and(warp::filters::body::bytes())
        .and(warp::path::tail())
        .and(with_aggregator(aggregator.clone()))
        .and_then(ingest_metrics);

    let mut get_metrics_headers = HeaderMap::new();
    get_metrics_headers.insert("Content-Type", HeaderValue::from_static("text/plain; version=0.0.4"));

    let get_metrics_path = warp::path!("metrics")
        .and(warp::get())
        .and(with_aggregator(aggregator.clone()))
        .and_then(get_metrics)
        .with(warp::reply::with::headers(get_metrics_headers));

    return push_metrics_path.or(get_metrics_path);
}

fn with_aggregator(
    agg: Aggregator,
) -> impl Filter<Extract = (Aggregator,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || agg.clone())
}

/// The routes for POST /metrics requests - takes a Prometheus exposition format
/// and merges it into the existing metrics. Also supports push gateway syntax - /metrics/job/foo
/// adds a job="foo" label to all the metrics
async fn ingest_metrics(
    data: Bytes,
    tail: Tail,
    mut agg: Aggregator
) -> Result<impl warp::Reply, warp::Rejection> {
    let labels = {
        let mut labelset = HashMap::new();
        let mut labels = tail.as_str().split("/").peekable();
        while labels.peek().is_some() {
            let name = labels.next().unwrap();
            if name.is_empty() {
                break;
            }
            let value = labels.next().unwrap_or_default();
            labelset.insert(name, value);
        }
        labelset
    };

    let body = match String::from_utf8(data.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            return Err(warp::reject::custom(GravelError::Error("Invalid UTF-8 in body".into())));
        }
    };

    match agg.parse_and_merge(&body, &labels).await {
        Ok(_) => Ok(""),
        Err(e) => Err(warp::reject::custom(GravelError::AggregationError(e))),
    }
}

async fn get_metrics(agg: Aggregator) -> Result<impl warp::Reply, warp::Rejection> {
    Ok(format!("{}", agg.to_string().await))
}