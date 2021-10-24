use std::net::ToSocketAddrs;

use aggregator::Aggregator;
use clap::{App, Arg};
use slog::{Drain, error, info, o};

mod aggregator;
mod routes;

#[cfg(test)]
mod aggregator_test;

#[tokio::main]
async fn main() {
    let agg = Aggregator::new();

    let matches = App::new("notes-thing backend")
        .arg(
            Arg::with_name("listen")
                .short("l")
                .help("The address/port to listen on")
                .takes_value(true)
                .default_value("localhost:4278"),
        )
        .get_matches();

    let drain = slog_async::Async::new(slog_json::Json::new(std::io::stdout())
        .add_default_keys()
        .build()
        .fuse()).build().fuse();

    let log = slog::Logger::root(drain, o!());

    // Parse out the listen address
    let address = matches.value_of("listen").unwrap();
    let address: Vec<_> = match address.to_socket_addrs() {
        Ok(addr) => addr.collect(),
        Err(e) => {
            error!(log, "Failed to parse socket address from {}: {}", address, e);
            return;
        }
    };

    info!(log, "Listening on: {:?}", address);

    let routes = routes::get_routes(agg);

    let futures = address
        .into_iter()
        .map(move |addr| warp::serve(routes.clone()).run(addr));

    tokio::join!(futures::future::join_all(futures));
}
