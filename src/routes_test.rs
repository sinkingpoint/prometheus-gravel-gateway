use std::net::SocketAddr;

use crate::{routes::{self, RoutesConfig}, aggregator::Aggregator, auth::pass_through_auth};
use tokio::time::sleep;

#[tokio::test]
async fn test_27() {
    // https://github.com/sinkingpoint/prometheus-gravel-gateway/issues/27

    let agg = Aggregator::new();
    let config = RoutesConfig{
        authenticator: Box::new(pass_through_auth()),
        #[cfg(feature="clustering")]
        cluster_conf: None
    };

    let routes = routes::get_routes(agg, config);
    let server = warp::serve(routes);
    let server = tokio::spawn(server.run(SocketAddr::V4("127.0.0.1:4278".parse().unwrap())));

    // wait a bit for the server to come up.
    sleep(tokio::time::Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    let res = client.post("http://127.0.0.1:4278/metrics/job/localhost%3A80").body("test_metric 1
").send().await.unwrap();
    assert_eq!(res.status(), 200);

    let res = client.get("http://127.0.0.1:4278/metrics").send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), "test_metric{job=\"localhost:80\"} 1\n");

    let res = client.post("http://127.0.0.1:4278/metrics/job/localhost:80").body("test_metric 2
").send().await.unwrap();
    assert_eq!(res.status(), 200);

    let res = client.get("http://127.0.0.1:4278/metrics").send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), "test_metric{job=\"localhost:80\"} 3\n");

    server.abort();
}