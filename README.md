# Gravel Gateway

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

With the counter value being replaced, the gauge value being sumed, and the version value completly replacing the old version. You'll auso note that the clearmode label is removed by the gateway - it's not included in the metrics exposed to the Prometheus scrape. In that way, this aggregating process is completly transparent to the Prometheus.

## Motivation
I [recently wrote](https://blog.sinkingpoint.com/posts/prometheus-for-faas/) about my frustrations with trying to orchestrate Prometheus in an FAAS (Functions-As-A-Service) system that will rename nameless.
My key frustration was that the number of semantics I was trying to extract from my Prometheus metrics was too much for the limited amount of data you can 
ship with them. In particular, there was three semantics I was trying to drive:

1. Aggregated Counters - Things like request counts. FAAS applications only process one request (in general), so each sends a 1 to the gateway and I want to aggregate that into a total request count across all the invocations
2. Non aggregated Gauges - It doesn't really make sense to aggregate Gauges in the general case, so I want to be able to send gauge values to the gateway and have them replace the old value (TODO: A rolling average would be nice)
3. Info values - Things like the build information. When a new labelset comes along for these metrics, I want to be able to replace all the old labelsets, e.g. upgrading from `{version="0.1"}` to `{version="0.2"}` should replace the `{version="0.1"}` labelset

Existing gateways, like the [prom-aggregation-gateway](https://github.com/weaveworks/prom-aggregation-gateway), or [pushgateway](https://github.com/prometheus/pushgateway) are all or nothing in regards to aggregation - the pushgateway does not aggregate at all, completly replacing values as they come in. The aggregation gateway is the opposite here - it aggregates everything. What I wanted was something that allows more flexibility in how metrics are aggregated. To that end, I wrote the Gravel Gateway
