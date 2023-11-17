use std::{net::ToSocketAddrs, path::PathBuf};

use aggregator::Aggregator;
use clap::{App, Arg};
use slog::{Drain, error, info, o};

use crate::{auth::pass_through_auth, routes::RoutesConfig};

mod aggregator;
mod routes;
mod pebble;

#[cfg(feature="clustering")]
mod clustering;

#[cfg(test)]
mod aggregator_test;
#[cfg(test)]
mod routes_test;
mod auth;

use tokio::signal;

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
            .long("peers-file")
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

    #[cfg(feature="clustering")]
    let mut cluster_conf = None;
    #[cfg(feature="clustering")]
    {
        let cluster_enabled = matches.is_present("cluster-enabled");
        if cluster_enabled {
            let self_url = matches.value_of("listen").unwrap().to_owned() + "/metrics";
            if let Some(peers) = matches.values_of("peers") {
                let peers = peers.map(|p| p.to_string()).collect();
                cluster_conf = Some(clustering::ClusterConfig::new_from_static(self_url, peers));
            }
            else if let Some(peers_file) = matches.value_of("peers-file") {
                match clustering::ClusterConfig::new_from_file(self_url, peers_file) {
                    Ok(c) => cluster_conf = Some(c),
                    Err(e) => {
                        error!(log, "Failed to load cluster config from file {}: {}", peers_file, e);
                        return;
                    }
                }
            }
            else if let Some(peers_srv) = matches.value_of("peers-srv") {
                match clustering::ClusterConfig::new_from_srv(self_url, peers_srv) {
                    Ok(c) => cluster_conf = Some(c),
                    Err(e) => {
                        error!(log, "Failed to load cluster config from SRV {}: {}", peers_srv, e);
                        return;
                    }
                }
            }
            else {
                error!(log, "Cluster enabled, but no peers specified");
                return;
            }
        }
    }

    let mut config = RoutesConfig{
        authenticator: Box::new(pass_through_auth()),
        #[cfg(feature="clustering")]
        cluster_conf
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

        tokio::select! {
            _ = signal::ctrl_c() => {},

            _ = futures::future::join_all(address.into_iter()
                .map(move |addr| warp::serve(routes.clone()).tls().key_path(tls_key).cert_path(tls_cert).run(addr))) => {}
        };
    }
    else {
        tokio::select! {
            _ = signal::ctrl_c() => {},

            _ = futures::future::join_all(address.into_iter()
            .map(move |addr| warp::serve(routes.clone()).run(addr))) => {}
        };
    };

    // If we don't have TLS support, just bind without it
    #[cfg(not(feature="tls"))]
    tokio::select! {
        _ = signal::ctrl_c() => {}
        _ = futures::future::join_all(address.into_iter().map(move |addr| warp::serve(routes.clone()).run(addr))) => {}
    };
}
