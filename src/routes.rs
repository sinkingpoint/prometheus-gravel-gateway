use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use warp::{Filter, hyper::body::Bytes, path::Tail, reject::Reject};

use crate::aggregator::{AggregationError, Aggregator};

#[derive(Debug)]
enum GravelError {
    Error(String),
    AggregationError(AggregationError)
}

impl Reject for GravelError {}

pub fn get_routes(aggregator: Aggregator) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let aggregator = Arc::new(RwLock::new(aggregator));
    let push_metrics_path = warp::path("metrics")
        .and(warp::post())
        .and(warp::filters::body::bytes())
        .and(warp::path::tail())
        .and(with_aggregator(aggregator.clone()))
        .and_then(ingest_metrics);

    return push_metrics_path;
}

fn with_aggregator(
    agg: Arc<RwLock<Aggregator>>,
) -> impl Filter<Extract = (Arc<RwLock<Aggregator>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || agg.clone())
}

async fn ingest_metrics(
    data: Bytes,
    tail: Tail,
    agg: Arc<RwLock<Aggregator>>
) -> Result<impl warp::Reply, warp::Rejection> {
    let labels = {
        let mut labelset = HashMap::new();
        let mut labels = tail.as_str().split("/").peekable();
        while labels.peek().is_some() {
            let name = labels.next().unwrap();
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

    let mut agg = agg.write().await;
    match agg.parse_and_merge(&body, &labels).await {
        Ok(_) => Ok(""),
        Err(e) => Err(warp::reject::custom(GravelError::AggregationError(e))),
    }
}
