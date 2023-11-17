# Gravel Gateway

![Crates.io](https://img.shields.io/crates/v/gravel-gateway?style=flat-square)

Gravel Gateway is a Prometheus Push Gateway for FAAS applications. In particular it allows aggregation to be controlled by the incoming metrics, and thus provides much more flexibility in the semantics that your metrics can follow. In general, the Gravel Gateway functions as a standard aggregating push gateway - by default, everything except Gauges are `sum`ed, so e.g. if you push 

```
# TYPE value_total counter
value_total 1
# TYPE value2 gauge
value2 1
```

three times, then Prometheus will scrape

```
# TYPE value_total counter
value_total 3
# TYPE value2 gauge
value2 1
```

Where the Gravel Gateway differs, is that it allows you to specify a special `clearmode` label to dictate how metrics are aggregated. 

We currently support three different values of `clearmode` - `aggregate` (the default for non gauges), `replace` (the default for gauges), and `family` which provides info like semantics. As a practical example, if we push:

```
# TYPE value_total counter
value_total 1
# TYPE value2 gauge
value2{clearmode="aggregate"} 1
# TYPE version gauge
version{version="0.0.1",clearmode="family"} 1
```

and then 

```
# TYPE value_total counter
value_total 3
# TYPE value2 gauge
value2{clearmode="aggregate"} 1
# TYPE version gauge
version{version="0.0.2",clearmode="family"} 1
```

(note the changed version label), Prometheus will scrape:

```
# TYPE version gauge
version{version="0.0.2"} 1
# TYPE value2 gauge
value2 2
# TYPE value_total counter
value_total 4
```

With the counter value being replaced, the gauge value being sumed, and the version value completely replacing the old version. You'll also note that the clearmode label is removed by the gateway - it's not included in the metrics exposed to the Prometheus scrape. In that way, this aggregating process is completely transparent to Prometheus.

## Usage

```
Prometheus Gravel Gateway 

USAGE:
    gravel-gateway [FLAGS] [OPTIONS]

FLAGS:
    --cluster-enabled    
        Whether or not to enable clustering

    -h, --help               
            Prints help information

    -V, --version            
            Prints version information


OPTIONS:
        --basic-auth-file <basic-auth-file>    
            The file to use for basic authentication validation.
                            This should be a path to a file of bcrypt hashes, one per line,
                            with each line being an allowed hash.
    -l <listen>                                
            The address/port to listen on [default: localhost:4278]

        --peer <peers>...                      
            The address/port of a peer to connect to

        --peers-file <peers-file>              
            The SRV record to look up to discover peers

        --peers-srv <peers-srv>                
            The SRV record to look up to discover peers

        --tls-cert <tls-cert>                  
            The certificate file to use with TLS

        --tls-key <tls-key>                    
            The private key file to use with TLS
```

To use, run the gateway:

```
gravel-gateway
```

You can then make POSTs to /metrics to push metrics:

```bash
echo '# TYPE value_total counter
value_total{clearmode="replace"} 3
# TYPE value2 gauge
value2{clearmode="aggregate"} 1
# TYPE version gauge
version{version="0.0.2",clearmode="family"} 1' | curl --data-binary @- localhost:4278/metrics
```

And point Prometheus at it to scrape:

```
global:
  scrape_interval: 15s
  evaluation_interval: 30s
scrape_configs:
  - job_name: prometheus
    honor_labels: true
    static_configs:
      - targets: ["127.0.0.1:4278"]
```

### Authentication

Gravel Gateway supports (pseudo) Basic authentication (with the auth feature). To use, populate a file with bcrypt hashes, 1 per line, e.g.

```bash
htpasswd -bnBC 10 "" supersecrets | tr -d ':\n' > passwords
```

and then start gravel-gateway pointing to that file:

```bash
gravel-gateway --basic-auth-file ./passwords
```

Requests to the POST /metrics endpoint will then be rejected unless they contain a valid `Authorization` header:

```
curl http://localhost:4278/metrics -vvv --data-binary @metrics.txt -u :supersecrets
```

### TLS

TLS is provided by the `tls-key` and `tls-cert` args. Both are required to start a TLS server, and represent the private key, and the certificate that is presented respectively.

### Clustering

To horizonally scale the gateway, you can use clustering. The Gravel Gateway support clustering by maintaining a hash ring of peers, provided by either a static list, an SRV record, or a file. When a request comes in, if clustering is enabled, the job label is hashed to produce an "authoritive" node for that job, and the request is forwarded accordingly. That node thus becomes the only node that will expose metrics for the given job.

To enable clustering, use the `cluster-enabled` flag, and provide a discovery mechanism. For example:

```
./gravel-gateway --cluster-enabled --peer localhost:4279 --peer localhost:4280
./gravel-gateway --cluster-enabled -l localhost:4279 --peer localhost:4278 --peer localhost:4280
./gravel-gateway --cluster-enabled -l localhost:4280 --peer localhost:4278 --peer localhost:4279
```

starts three gravel gateway instances, clustered such that they will forward requests between each other

### Pebbles

Some times, for Gauges, you don't want to track just one of your values (the default for Gauges is "replace"). If we have, say, a new release that doubles the memory usage, then we probably want to know about that increase without it being pulled down by weeks of the previous version. For this usecase, the Gravel Gateway supports "pebbles". Pebbles are effectively a circular buffer of time based buckets. Each bucket represents a distinct timeslice, and tracks a pre-aggregated value inside that time slice. The final value for the metric is the same aggregation applied over each bucket.

This means that we actually get an "aggregate of aggregates" out the other end, which does lose _some_ precision compared to storing all the raw data but in practice this hasn't affected us much. 

You can start a pebble using a clearmode in the form `<aggregation><time>` e.g. `{clearmode="mean5m"}` will take a mean over the last 5 minutes of incoming data. Available aggregations at the moment include "sum" and "mean", but "median" is coming soon, and maybe "percentile" would be a good PR.

## Motivation

I [recently wrote](https://blog.sinkingpoint.com/posts/prometheus-for-faas/) about my frustrations with trying to orchestrate Prometheus in an FAAS (Functions-As-A-Service) system that will rename nameless.
My key frustration was that the number of semantics I was trying to extract from my Prometheus metrics was too much for the limited amount of data you can 
ship with them. In particular, there was three semantics I was trying to drive:

1. Aggregated Counters - Things like request counts. FAAS applications only process one request (in general), so each sends a 1 to the gateway and I want to aggregate that into a total request count across all the invocations
2. Non aggregated Gauges - It doesn't really make sense to aggregate Gauges in the general case, so I want to be able to send gauge values to the gateway and have them replace the old value
3. Info values - Things like the build information. When a new labelset comes along for these metrics, I want to be able to replace all the old labelsets, e.g. upgrading from `{version="0.1"}` to `{version="0.2"}` should replace the `{version="0.1"}` labelset

Existing gateways, like the [prom-aggregation-gateway](https://github.com/weaveworks/prom-aggregation-gateway), or [pushgateway](https://github.com/prometheus/pushgateway) are all or nothing in regards to aggregation - the pushgateway does not aggregate at all, completly replacing values as they come in. The aggregation gateway is the opposite here - it aggregates everything. What I wanted was something that allows more flexibility in how metrics are aggregated. To that end, I wrote the Gravel Gateway

## How to publish a new image in ECR
```
export AWS_PROFILE=<aws-profile>
export AWS_ACCOUNT_ID=<aws-account-id>
export AWS_REGION=<ecr-region>
export DOCKER_IMAGE_NAME=${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com/prometheus-gravel-gateway
make ecr-login
make build DOCKER_IMAGE_NAME=${DOCKER_IMAGE_NAME}
make push DOCKER_IMAGE_NAME=${DOCKER_IMAGE_NAME}
```
