use server::rate_limit::LeakyBucket;
use std::thread::sleep;
use std::time::Duration;

#[test]
fn bucket_blocks_until_refill() {
    let mut bucket = LeakyBucket::new(2.0, 2.0);
    assert!(bucket.allow(1.0));
    assert!(bucket.allow(1.0));
    assert!(!bucket.allow(1.0));
    sleep(Duration::from_millis(600));
    assert!(bucket.allow(1.0));
}
