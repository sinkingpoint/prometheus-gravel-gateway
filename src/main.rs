use aggregator::Aggregator;

mod aggregator;

#[tokio::main]
async fn main() {
    let mut agg = Aggregator::new();
    agg.parse_and_merge("# HELP test test metric\ntest 1\n")
        .await
        .unwrap();
    agg.parse_and_merge("# HELP test test metric\ntest 1\n")
        .await
        .unwrap();
    agg.parse_and_merge("# HELP test test metric\ntest 1\n")
        .await
        .unwrap();
    agg.parse_and_merge("# HELP test2 test metric\n# TYPE test2 gauge\ntest2 1\n")
        .await
        .unwrap();
    agg.parse_and_merge("# HELP test2 test metric\n# TYPE test2 gauge\ntest2 1\n")
        .await
        .unwrap();

    println!("{:?}", agg);
}
