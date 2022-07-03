This crate implements a cache with frequency-based replacement strategy as described in the paper
“Data Cache Management Using Frequency-Based Replacement” by John T. Robinson and Murthy V. Devarakonda,
published in ACM SIGMETRICS 1990.

The configuration parameters of such a cache are:

- **capacity:** the number of slots
- **A_max:** the maximum average frequency count beyond which counts are aged.
  Aging will be performed roughly every _A_max * capacity_ cache hits.
  100 is a reasonable default.
- **C_max:** the maximum frequency count for which eviction happens by count (above that: LRU).
  You will probably not need to tune this.

Example:

```rust
use fbr_cache::FbrCache;

let mut cache = FbrCache::new(1000);
cache.put(1, "hello");
cache.put(2, "world");

assert_eq!(cache.get(&1), Some(&"hello"));
assert_eq!(cache.len(), 2);
```
