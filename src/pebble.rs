
use std::{time::{Duration, SystemTime}, fmt};

type MergeStrategy = fn(old: &PebbleEntry, new: &PebbleEntry) -> f64;

pub fn sum_merge_strategy(old: &PebbleEntry, new: &PebbleEntry) -> f64 {
    old.value + new.value
}

pub fn mean_merge_strategy(old: &PebbleEntry, new: &PebbleEntry) -> f64 {
    let top = (old.weight as f64 * old.value) + (new.weight as f64 * new.value);
    let bottom = old.weight + new.weight;

    if bottom == 0 {
        return 0.
    }

    top / bottom as f64
}

#[derive(Debug, Clone, PartialEq)]
pub struct PebbleEntry {
    weight: i32,
    value: f64,
}

impl PebbleEntry {
    fn reset(&mut self) {
        self.weight = 0;
        self.value = 0.0;
    }
}

#[derive(Clone)]
pub struct TimePebble {
    buckets: Vec<PebbleEntry>,
    merge: MergeStrategy,
    bucket_size_nanos: u128,
    last_bucket_index: usize,
    last_bucket_time_nanos: u128,
}

impl fmt::Debug for TimePebble {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimePebble").field("buckets", &self.buckets).field("bucket_size_nanos", &self.bucket_size_nanos).field("last_bucket_index", &self.last_bucket_index).field("last_bucket_time_nanos", &self.last_bucket_time_nanos).finish()
    }
}

impl PartialEq for TimePebble {
    fn eq(&self, other: &Self) -> bool {
        self.buckets == other.buckets && self.bucket_size_nanos == other.bucket_size_nanos && self.last_bucket_index == other.last_bucket_index && self.last_bucket_time_nanos == other.last_bucket_time_nanos
    }
}

impl TimePebble {
    pub fn new(time_span: Duration, granularity: usize, merge: MergeStrategy) -> TimePebble {
        return TimePebble {
            buckets: vec![PebbleEntry { weight: 0, value: 0. }; 100],
            merge,
            bucket_size_nanos: time_span.as_nanos() / granularity as u128,
            last_bucket_index: 0,
            last_bucket_time_nanos: 0,
        }
    }

    fn reset_window(&mut self) {
        for bucket in &mut self.buckets {
            bucket.reset();
        }
    }

    fn reset_buckets(&mut self, window_offset: usize) {
        let mut distance = window_offset as isize - self.last_bucket_index as isize;
        // 	// If the distance between current and last is negative then we've wrapped
        // 	// around the ring. Recalculate the distance.
        if distance < 0 {
            distance = (self.buckets.len() as isize - self.last_bucket_index as isize) + window_offset as isize
        }

        for i in 1..distance as usize {
            let offset = (self.last_bucket_index + i) % self.buckets.len();
            self.buckets[offset].reset();
        }
    }

    fn keep_consistent(&mut self, adjusted_time: u128, window_offset: usize) {
        // If the time is before the last bucket, then we need to reset the window
        if adjusted_time - self.last_bucket_time_nanos > self.buckets.len() as u128 {
            self.reset_window();
        }

        // When one or more buckets are missed we need to zero them out.
        if adjusted_time != self.last_bucket_time_nanos && adjusted_time - self.last_bucket_time_nanos < self.buckets.len() as u128 {
            self.reset_buckets(window_offset);
        }
    }

    fn select_bucket(&self, time: SystemTime) -> (u128, usize) {
        let adjusted_time = time.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() / self.bucket_size_nanos;
        let window_offset = (adjusted_time % self.buckets.len() as u128) as usize;

        return (adjusted_time, window_offset)
    }

    pub fn append_with_timestamp(&mut self, value: f64, timestamp: SystemTime) {
        let (adjusted_time, window_offset) = self.select_bucket(timestamp);
        self.keep_consistent(adjusted_time, window_offset);

        self.buckets[window_offset].weight += 1;
        self.buckets[window_offset].value = (self.merge)(&self.buckets[window_offset], &PebbleEntry {
            weight: 1,
            value,
        });

        self.last_bucket_time_nanos = adjusted_time;
        self.last_bucket_index = window_offset;
    }

    pub fn append(&mut self, value: f64) {
        self.append_with_timestamp(value, SystemTime::now())
    }

    pub fn aggregate(&self) -> f64 {
        let mut pebble_value = PebbleEntry{
            weight: 0,
            value: 0.0,
        };

        for bucket in &self.buckets {
            if bucket.weight == 0 {
                continue
            }

            pebble_value = PebbleEntry {
                weight: pebble_value.weight + bucket.weight,
                value: (self.merge)(&pebble_value, bucket)
            }
        }

        return (self.merge)(&pebble_value, &PebbleEntry {
            weight: 0,
            value: 0.0,
        });
    }
}

pub fn parse_duration(s: &str) -> Option<Duration> {
    let magnitude = match s.chars().filter(|c| c.is_numeric()).collect::<String>().parse() {
        Ok(m) => m,
        Err(_) => return None,
    };
    
    match s.chars().last() {
        Some('s') => Some(Duration::from_secs(magnitude)),
        Some('m') => Some(Duration::from_secs(magnitude * 60)),
        Some('h') => Some(Duration::from_secs(magnitude * 60 * 60)),
        _ => return None
    }
}