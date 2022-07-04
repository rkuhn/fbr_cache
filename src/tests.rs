use crate::{FbrCache, Region};
use std::sync::atomic::{AtomicUsize, Ordering};

fn s(s: &str) -> String {
    s.to_owned()
}

#[test]
fn smoke() {
    let mut cache = FbrCache::<u32, String, 3>::with_age_threshold(10, 4);

    assert_eq!(cache.get(&1), None);
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
    assert_eq!(cache.iter().collect::<Vec<_>>(), vec![]);

    for i in 0..10 {
        cache.put(i, i.to_string());
    }
    assert_eq!(cache.get(&1), Some(&"1".to_owned()));
    assert_eq!(cache.len(), 10);
    assert!(!cache.is_empty());
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&1, &s("1"), 1, Region::New),
            (&9, &s("9"), 0, Region::New),
            (&8, &s("8"), 0, Region::New),
            (&7, &s("7"), 0, Region::Middle),
            (&6, &s("6"), 0, Region::Middle),
            (&5, &s("5"), 0, Region::Middle),
            (&4, &s("4"), 0, Region::Middle),
            (&3, &s("3"), 0, Region::Old),
            (&2, &s("2"), 0, Region::Old),
            (&0, &s("0"), 0, Region::Old)
        ]
    );

    cache.put(10, "10".to_string());
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&10, &s("10"), 0, Region::New),
            (&1, &s("1"), 1, Region::New),
            (&9, &s("9"), 0, Region::New),
            (&8, &s("8"), 0, Region::Middle),
            (&7, &s("7"), 0, Region::Middle),
            (&6, &s("6"), 0, Region::Middle),
            (&5, &s("5"), 0, Region::Middle),
            (&4, &s("4"), 0, Region::Old),
            (&3, &s("3"), 0, Region::Old),
            (&2, &s("2"), 0, Region::Old),
        ]
    );
}

#[test]
fn evict_1() {
    let mut cache = FbrCache::<u32, String, 3>::with_age_threshold(5, 4);
    cache.put(0, "0".to_owned());
    println!("{:?}", cache.iter().collect::<Vec<_>>());
    cache.put(1, "1".to_owned());
    println!("{:?}", cache.iter().collect::<Vec<_>>());
    cache.get(&0);
    println!("{:?}", cache.iter().collect::<Vec<_>>());
    for i in 0..5 {
        cache.put(i, i.to_string());
        println!("{:?}", cache.iter().collect::<Vec<_>>());
    }
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&4, &s("4"), 0, Region::New),
            (&3, &s("3"), 0, Region::Middle),
            (&2, &s("2"), 0, Region::Middle),
            (&1, &s("1"), 1, Region::Old),
            (&0, &s("0"), 1, Region::Old)
        ]
    );
    cache.get(&1);
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&1, &s("1"), 2, Region::New),
            (&4, &s("4"), 0, Region::Middle),
            (&3, &s("3"), 0, Region::Middle),
            (&2, &s("2"), 0, Region::Old),
            (&0, &s("0"), 1, Region::Old)
        ]
    );
    cache.put(5, "5".to_string());
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&5, &s("5"), 0, Region::New),
            (&1, &s("1"), 2, Region::Middle),
            (&4, &s("4"), 0, Region::Middle),
            (&3, &s("3"), 0, Region::Old),
            (&0, &s("0"), 1, Region::Old),
        ]
    );
}

#[test]
fn evict_3() {
    let mut cache = FbrCache::<u32, String, 3>::with_age_threshold(5, 4);
    for _ in 0..4 {
        for i in 0..5 {
            cache.put(i, i.to_string());
        }
    }
    cache.put(5, "5".to_string());
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&5, &s("5"), 0, Region::New),
            (&4, &s("4"), 3, Region::Middle),
            (&3, &s("3"), 3, Region::Middle),
            (&2, &s("2"), 3, Region::Old),
            (&1, &s("1"), 3, Region::Old)
        ]
    );
}

#[test]
fn aging() {
    let mut cache = FbrCache::<u32, String, 3>::with_age_threshold(5, 4);
    for n in 0usize..5 {
        for i in 1..6 {
            cache.put(i, i.to_string());
            assert_eq!(
                cache.total_count,
                (n * 5 + i as usize).saturating_sub(5),
                "n={} i={}",
                n,
                i
            );
        }
    }
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&5, &s("5"), 4, Region::New),
            (&4, &s("4"), 4, Region::Middle),
            (&3, &s("3"), 4, Region::Middle),
            (&2, &s("2"), 4, Region::Old),
            (&1, &s("1"), 4, Region::Old)
        ]
    );
    assert_eq!(cache.total_count, 20);

    cache.get(&1);
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&1, &s("1"), 2, Region::New),
            (&5, &s("5"), 2, Region::Middle),
            (&4, &s("4"), 2, Region::Middle),
            (&3, &s("3"), 2, Region::Old),
            (&2, &s("2"), 2, Region::Old),
        ]
    );
    assert_eq!(cache.total_count, 10);

    for _ in 0..4 {
        for i in 1..4 {
            cache.put(i, i.to_string());
        }
    }
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&3, &s("3"), 3, Region::New),
            (&2, &s("2"), 3, Region::Middle),
            (&1, &s("1"), 2, Region::Middle),
            (&5, &s("5"), 1, Region::Old),
            (&4, &s("4"), 1, Region::Old),
        ]
    );
}

#[test]
fn clear() {
    struct X<'a>(&'a AtomicUsize);
    impl<'a> Drop for X<'a> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    let counter = AtomicUsize::new(0);
    let mut cache = FbrCache::<u32, X, 3>::with_age_threshold(5, 4);
    for i in 0..6 {
        cache.put(i, X(&counter));
    }
    assert_eq!(counter.load(Ordering::Relaxed), 1);
    cache.clear();
    assert_eq!(counter.load(Ordering::Relaxed), 6);
    for i in 0..6 {
        cache.put(i, X(&counter));
    }
    assert_eq!(counter.load(Ordering::Relaxed), 7);
    drop(cache);
    assert_eq!(counter.load(Ordering::Relaxed), 12);
}

#[test]
fn prio() {
    let mut cache = FbrCache::<u32, String, 3>::with_age_threshold(5, 4);
    cache.put_prio(0, 0.to_string());
    for i in 1..6 {
        cache.put(i, i.to_string());
    }
    assert_eq!(
        cache.iter().collect::<Vec<_>>(),
        vec![
            (&5, &s("5"), 0, Region::New),
            (&4, &s("4"), 0, Region::Middle),
            (&3, &s("3"), 0, Region::Middle),
            (&2, &s("2"), 0, Region::Old),
            (&0, &s("0"), 1, Region::Old),
        ]
    );
}
