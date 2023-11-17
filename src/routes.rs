use std::{collections::HashMap, sync::Arc, convert::Infallible};

use reqwest::StatusCode;
use urlencoding::decode;
use warp::{Filter, http::HeaderValue, hyper::{HeaderMap, body::Bytes}, path::Tail, reject::Reject};

use crate::{aggregator::{AggregationError, Aggregator}, auth::Authenticator};

#[cfg(feature="clustering")]
use crate::clustering::ClusterConfig;

#[derive(Debug)]
enum GravelError {
    Error(String),
    AuthError,
    AggregationError(AggregationError)
}

impl Reject for GravelError {}

pub struct RoutesConfig {
    pub authenticator: Box<dyn Authenticator + Send + Sync>,
    #[cfg(feature="clustering")]
    pub cluster_conf: Option<ClusterConfig>
}

async fn auth(config: Arc<RoutesConfig>, header: String) -> Result<(), warp::Rejection> {
    if let Ok(true) = config.authenticator.authenticate(&header) {
        return Ok(());
    }

    return Err(warp::reject::custom(GravelError::AuthError));
}

pub fn get_routes(aggregator: Aggregator, config: RoutesConfig) -> impl Filter<Extract = impl warp::Reply, Error = Infallible> + Clone {
    let default_auth = warp::any().map(|| {
        return String::new();
    });

    let config = Arc::new(config);
    let auth_config = Arc::clone(&config);

    let auth = warp::header::<String>("authorization").or(default_auth).unify().and_then(move |header| auth(auth_config.clone(), header)).untuple_one();

    let push_metrics_path = warp::path("metrics")
        .and(warp::post().or(warp::put()))
        .and(auth)
        .and(warp::filters::body::bytes())
        .and(warp::path::tail())
        .and(with_aggregator(aggregator.clone()))
        .and(with_config(Arc::clone(&config)))
        .and_then(ingest_metrics);

    let mut get_metrics_headers = HeaderMap::new();
    get_metrics_headers.insert("Content-Type", HeaderValue::from_static("text/plain; version=0.0.4"));

    let get_metrics_path = warp::path!("metrics")
        .and(warp::get())
        .and(with_aggregator(aggregator.clone()))
        .and_then(get_metrics)
        .with(warp::reply::with::headers(get_metrics_headers));

    return push_metrics_path.or(get_metrics_path).recover(handle_rejection);
}

async fn handle_rejection(err: warp::Rejection) -> Result<impl warp::Reply, std::convert::Infallible> {
    let gravel_error: Option<&GravelError> = err.find();
    match gravel_error {
        Some(GravelError::AuthError) => Ok(warp::reply::with_status(String::from("FORBIDDEN"), StatusCode::FORBIDDEN)),
        Some(GravelError::AggregationError(err)) => Ok(warp::reply::with_status(err.to_string(), StatusCode::BAD_REQUEST)),
        Some(GravelError::Error(err)) => Ok(warp::reply::with_status(err.clone(), StatusCode::BAD_REQUEST)),
        None => Ok(warp::reply::with_status(String::new(), StatusCode::NOT_FOUND)),
    }
}

fn with_aggregator(
    agg: Aggregator,
) -> impl Filter<Extract = (Aggregator,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || agg.clone())
}

fn with_config(
    conf: Arc<RoutesConfig>,
) -> impl Filter<Extract = (Arc<RoutesConfig>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || Arc::clone(&conf))
}

#[cfg(feature="clustering")]
async fn forward_to_peer(peer: &str, data: Bytes, url_tail: Tail) -> Result<(), GravelError> {
    let client = reqwest::Client::new();
    return match client.post(peer.to_owned() + "/" + url_tail.as_str()).body(data).send().await {
        Ok(o) => {
            if o.status().is_success() {
                return Ok(());
            }

            return Err(GravelError::Error(format!("Failed to forward to peer. Got status: {}", 200)));
        },
        Err(e) => Err(GravelError::Error(e.to_string()))
    }
}

/// The routes for POST /metrics requests - takes a Prometheus exposition format
/// and merges it into the existing metrics. Also supports push gateway syntax - /metrics/job/foo
/// adds a job="foo" label to all the metrics
async fn ingest_metrics<T>(
    _method: T,
    data: Bytes,
    url_tail: Tail,
    mut agg: Aggregator,
    conf: Arc<RoutesConfig>
) -> Result<impl warp::Reply, warp::Rejection> {
    let labels = {
        let mut labelset = HashMap::new();
        let mut labels = url_tail.as_str().split("/").map(|s| decode(s)).peekable();
        while labels.peek().is_some() {
            let label_name = labels.next().unwrap();
            let name = match label_name {
                Ok(s) => s.into_owned(),
                Err(_) => return Err(warp::reject::custom(GravelError::Error("Invalid label name".into())))
            };

            if name.is_empty() {
                break;
            }

            let value = match labels.next() {
                Some(Ok(s)) => s.into_owned(),
                Some(Err(_)) => return Err(warp::reject::custom(GravelError::Error("Invalid label value".into()))),
                None => return Err(warp::reject::custom(GravelError::Error("Label value missing".into())))
            };

            labelset.insert(name, value);
        }
        labelset
    };

    // We're clustering, so might need to forward the metrics
    if let Some(cluster_conf) = conf.cluster_conf.as_ref() {
        let job = labels.get("job").map(|s| s.to_owned()).unwrap_or(String::new());
        if let Some(peer) = cluster_conf.get_peer_for_key(&job) {
            if !cluster_conf.is_self(peer) {
                match forward_to_peer(peer, data, url_tail).await {
                    Ok(_) => return Ok(""),
                    Err(e) => return Err(warp::reject::custom(e))
                }
            }
        }
    }

    let body = match String::from_utf8(data.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            return Err(warp::reject::custom(GravelError::Error("Invalid UTF-8 in body".into())));
        }
    };

    let mut str_labels = HashMap::new();
    for (k, v) in labels.iter() {
        str_labels.insert(k.as_str(), v.as_str());
    }

    match agg.parse_and_merge(&body, &str_labels).await {
        Ok(_) => Ok(""),
        Err(e) => Err(warp::reject::custom(GravelError::AggregationError(e))),
    }
}

async fn get_metrics(agg: Aggregator) -> Result<impl warp::Reply, warp::Rejection> {
    Ok(agg.to_string().await)
}