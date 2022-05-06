use std::{net::ToSocketAddrs, path::PathBuf};

use aggregator::Aggregator;
use clap::{App, Arg};
use slog::{Drain, error, info, o};

use crate::{auth::pass_through_auth, routes::RoutesConfig};

mod aggregator;
mod routes;

#[cfg(test)]
mod aggregator_test;
mod auth;

#[tokio::main]
async fn main() {
    let agg = Aggregator::new();

    let app = App::new("Prometheus Gravel Gateway")
        .arg(
            Arg::with_name("listen")
                .short("l")
                .help("The address/port to listen on")
                .takes_value(true)
                .default_value("localhost:4278"),
        );
    

    #[cfg(feature="clustering")]
    let app = app.arg(
        Arg::with_name("cluster-enabled")
            .long("cluster-enabled")
            .help("Whether or not to enable clustering")
    );
    
    #[cfg(feature="clustering")]
    let app = app.arg(
        Arg::with_name("peers")
            .long("peer")
            .takes_value(true)
            .multiple(true)
            .requires("cluster-enabled")
            .help("The address/port of a peer to connect to")
    );

    #[cfg(feature="clustering")]
    let app = app.arg(
        Arg::with_name("peers-srv")
            .long("peers-srv")
            .takes_value(true)
            .requires("cluster-enabled")
            .help("The SRV record to look up to discover peers")
    );

    #[cfg(feature="clustering")]
    let app = app.arg(
        Arg::with_name("peers-file")
            .long("peers-srv")
            .takes_value(true)
            .requires("cluster-enabled")
            .help("The SRV record to look up to discover peers")
    );

    #[cfg(feature="tls")]
    let app = app.arg(
        Arg::with_name("tls-key")
            .long("tls-key")
            .help("The private key file to use with TLS")
            .requires("tls-cert")
            .takes_value(true)
    )
    .arg(
        Arg::with_name("tls-cert")
            .long("tls-cert")
            .help("The certificate file to use with TLS")
            .requires("tls-key")
            .takes_value(true)
    );

    #[cfg(feature="auth")]
    let app = app.arg(
        Arg::with_name("basic-auth-file")
            .long("basic-auth-file")
            .help("The file to use for basic authentication validation")
            .long_help(
                "The file to use for basic authentication validation.
                This should be a path to a file of bcrypt hashes, one per line,
                with each line being an allowed hash."
            )
            .takes_value(true)
    );
    
    let matches = app.get_matches();

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

    let mut config = RoutesConfig{
        authenticator: Box::new(pass_through_auth())
    };

    #[cfg(feature = "auth")]
    {
        use auth::basic_auth;
        if let Some(path) = matches.value_of("basic-auth-file") {
            config = match basic_auth(PathBuf::from(path)) {
                Ok(authenticator) => RoutesConfig {
                    authenticator: Box::new(authenticator),
                    cluster_conf: None
                },
                Err(e) => {
                    error!(log, "Failed to load basic auth file ({}) - {}", path, e);
                    return;
                }
            };
        };
    }
    
    let routes = routes::get_routes(agg, config);

    #[cfg(feature="tls")]
    if let Some(tls_key) = matches.value_of("tls-key") {
        // Clap ensures that if one of these exists, so does the other
        let tls_cert = matches.value_of("tls-cert").unwrap();
        tokio::join!(futures::future::join_all(address
            .into_iter()
            .map(move |addr| warp::serve(routes.clone()).tls().key_path(tls_key).cert_path(tls_cert).run(addr))));
    }
    else {
        tokio::join!(futures::future::join_all(address
            .into_iter()
            .map(move |addr| warp::serve(routes.clone()).run(addr))));
    };

    // If we don't have TLS support, just bind without it
    #[cfg(not(feature="tls"))]
    tokio::join!(futures::future::join_all(address
        .into_iter()
        .map(move |addr| warp::serve(routes.clone()).run(addr))));
}
