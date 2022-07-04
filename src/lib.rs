#![doc = include_str!("../README.md")]

use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink, UnsafeRef};
use std::{collections::HashMap, hash::Hash, ptr::null};

#[cfg(test)]
mod tests;

/// Region in which a cache entry currently lives
///
/// New inhibits frequency count (counting one “run” as 1),
/// Old is where eviction happens.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Region {
    New,
    Middle,
    Old,
}

#[derive(Debug)]
struct FbrEntry<K, V> {
    lru: LinkedListLink,
    chain: LinkedListLink,
    count: usize,
    region: Region,
    key: K,
    value: V,
}

impl<K, V> FbrEntry<K, V> {
    fn new(key: K, value: V) -> Self {
        Self {
            lru: Default::default(),
            chain: Default::default(),
            count: 0,
            region: Region::New,
            key,
            value,
        }
    }
    pub fn reuse(ptr: &UnsafeRef<Self>, key: K, value: V) {
        let this = unsafe { &mut *UnsafeRef::into_raw(ptr.clone()) };
        this.count = 0;
        this.region = Region::New;
        this.key = key;
        this.value = value;
    }
    pub fn access(ptr: &UnsafeRef<Self>) -> usize {
        let this = unsafe { &mut *UnsafeRef::into_raw(ptr.clone()) };
        let count = this.count;
        if this.region != Region::New {
            this.count += 1;
        }
        this.region = Region::New;
        count
    }
    pub fn bump(ptr: &UnsafeRef<Self>) {
        let this = unsafe { &mut *UnsafeRef::into_raw(ptr.clone()) };
        this.count += 1;
    }
    pub fn age(ptr: &UnsafeRef<Self>) -> usize {
        let this = unsafe { &mut *UnsafeRef::into_raw(ptr.clone()) };
        let count = this.count;
        this.count /= 2;
        count - this.count
    }
    pub fn region(ptr: &UnsafeRef<Self>, region: Region) {
        let this = unsafe { &mut *UnsafeRef::into_raw(ptr.clone()) };
        this.region = region;
    }
}

intrusive_adapter!(ListLru<K, V> = UnsafeRef<FbrEntry<K, V>>: FbrEntry<K, V> { lru: LinkedListLink });
intrusive_adapter!(ListChain<K, V> = UnsafeRef<FbrEntry<K, V>>: FbrEntry<K, V> { chain: LinkedListLink });

/// Cache with frequency-based replacement strategy.
///
/// Items are held in recently-used order, with the front 30% of the list
/// designated as “new” space and the back 25% as “old” space. Each cache hit
/// (via [`Self::get`] or [`Self::put`]) moves the item in question to the front
/// of the list and increments the usage count if the item was not in the “new”
/// space before.
///
/// Usage counts are periodically aged (halved) to prevent items that were popular
/// in the past from staying in the cache forever. This happens when the average
/// frequency count exceeds the `age_threshold` parameter.
///
/// Eviction only removes “old” items: if there are some with usage count less
/// than `C_MAX`, the least recent among the least-used ones is taken; otherwise
/// the least recently used is evicted.
///
/// The cache will allocate only during the initial filling phase, afterwards it
/// reuses the heap allocations where values are held.
///
/// ## Requirements
///
/// - `C_MAX` must be at least 2
/// - `capacity` must be at least 4
/// - `age_threshold` must be at least 1
pub struct FbrCache<K, V, const C_MAX: usize> {
    hash: HashMap<K, UnsafeRef<FbrEntry<K, V>>>,
    lru: LinkedList<ListLru<K, V>>,
    chains: [LinkedList<ListChain<K, V>>; C_MAX],
    mid: usize,
    mid_boundary: Option<UnsafeRef<FbrEntry<K, V>>>,
    old: usize,
    old_boundary: Option<UnsafeRef<FbrEntry<K, V>>>,
    total_count: usize,
    capacity: usize,
    age_threshold: usize,
}

impl<K, V, const C: usize> Drop for FbrCache<K, V, C> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<K, V, const C: usize> std::fmt::Debug for FbrCache<K, V, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FbrCache")
            .field("capacity", &self.capacity)
            .field("items", &self.hash.len())
            .field("total_count", &self.total_count)
            .field("age_threshold", &self.age_threshold)
            .finish()
    }
}

impl<K, V, const C: usize> FbrCache<K, V, C> {
    /// The number of items currently in the cache.
    pub fn len(&self) -> usize {
        self.hash.len()
    }

    /// Returns `true` if there are no items in the cache.
    pub fn is_empty(&self) -> bool {
        self.hash.is_empty()
    }

    /// Clears all items from the cache.
    pub fn clear(&mut self) {
        self.lru.fast_clear();
        for chain in &mut self.chains {
            chain.fast_clear();
        }
        self.mid_boundary = None;
        self.old_boundary = None;
        self.total_count = 0;
        for (_, cde) in self.hash.drain() {
            unsafe { UnsafeRef::into_box(cde) };
        }
    }

    /// An iterator over all currently held items together with their usage count and region.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V, usize, Region)> {
        self.lru
            .iter()
            .map(|cde| (&cde.key, &cde.value, cde.count, cde.region))
    }
}

impl<K: Hash + Eq + Clone, V> FbrCache<K, V, 8> {
    /// Create a new cache with the given capacity and aging threshold.
    pub fn new(capacity: usize) -> Self {
        Self::with_age_threshold(capacity, 100)
    }
}
impl<K: Hash + Eq + Clone, V, const C: usize> FbrCache<K, V, C> {
    /// Create a new cache with the given capacity and aging threshold.
    pub fn with_age_threshold(capacity: usize, age_threshold: usize) -> Self {
        Self {
            hash: Default::default(),
            lru: Default::default(),
            chains: [(); C].map(|_| Default::default()),
            mid: capacity * 3 / 10,
            mid_boundary: None,
            old: capacity * 3 / 4,
            old_boundary: None,
            total_count: Default::default(),
            capacity,
            age_threshold: capacity.saturating_mul(age_threshold),
        }
    }

    /// Put the given item into the cache, evicting another item if necessary.
    ///
    /// This is usually called after finding no cached value for a key and computing said value.
    pub fn put(&mut self, key: K, value: V) {
        if self.get(&key).is_some() {
            return;
        }
        self.insert(key, value, false);
    }

    /// Put the given item into the cache with elevated priority.
    ///
    /// This means that the item starts out with a usage count of one instead
    /// of zero. For cyclic usage patterns this means that priority items will
    /// accumulate in the “old” region since non-priority items are evicted
    /// before them. As usual, this works best if only a small fraction of
    /// items get priority.
    pub fn put_prio(&mut self, key: K, value: V) {
        if self.get(&key).is_some() {
            return;
        }
        self.insert(key, value, true);
    }

    /// Retrieve the value for a given key
    ///
    /// This updates the usage count and recency, so it can be used to “ping” a
    /// key in order to bring it to the front again.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if let Some(cde) = self.hash.get(key) {
            let region = cde.region;
            let old_count = FbrEntry::access(cde);
            let new_count = cde.count;
            switch_chain(old_count, new_count, &mut self.chains, cde);
            unsafe {
                let mut cursor = self.lru.cursor_mut_from_ptr(cde.as_ref());
                if optr(&self.mid_boundary) == ptr(cde) {
                    self.mid_boundary = cursor.peek_next().clone_pointer();
                } else if optr(&self.old_boundary) == ptr(cde) {
                    self.old_boundary = cursor.peek_next().clone_pointer();
                }
                cursor.remove();
            };
            self.lru.push_front(cde.clone());
            move_boundaries(
                region,
                self.len(),
                self.mid,
                self.old,
                &self.lru,
                &mut self.mid_boundary,
                &mut self.old_boundary,
            );

            // periodic aging
            self.total_count += new_count - old_count;
            if self.total_count > self.age_threshold {
                for cde in self.lru.iter() {
                    let ptr = unsafe { UnsafeRef::from_raw(cde) };
                    let old_count = ptr.count;
                    self.total_count -= FbrEntry::age(&ptr);
                    switch_chain(old_count, ptr.count, &mut self.chains, &ptr);
                }
            }

            Some(&cde.value)
        } else {
            None
        }
    }

    fn insert(&mut self, key: K, value: V, prio: bool) {
        let entry = if self.len() >= self.capacity {
            let e = self.evict();
            FbrEntry::reuse(&e, key.clone(), value);
            e
        } else {
            UnsafeRef::from_box(Box::new(FbrEntry::new(key.clone(), value)))
        };
        if prio {
            FbrEntry::bump(&entry);
        }
        self.hash.insert(key, entry.clone());
        self.lru.push_front(entry.clone());
        move_boundaries(
            Region::Old,
            self.len(),
            self.mid,
            self.old,
            &self.lru,
            &mut self.mid_boundary,
            &mut self.old_boundary,
        );
        self.chains[entry.count].push_front(entry);
    }

    fn evict(&mut self) -> UnsafeRef<FbrEntry<K, V>> {
        let mut found = None;
        for chain in &mut self.chains {
            let ptr = chain.back().clone_pointer();
            if let Some(cde) = ptr {
                if cde.region == Region::Old {
                    unsafe { chain.cursor_mut_from_ptr(cde.as_ref()) }.remove();
                    found = Some(cde);
                    break;
                }
            }
        }
        // in case old region didn’t contain anything in self.chains, evict LRU
        let cde = found.unwrap_or_else(|| self.lru.back().clone_pointer().unwrap());
        unsafe {
            let mut cursor = self.lru.cursor_mut_from_ptr(cde.as_ref());
            if optr(&self.mid_boundary) == ptr(&cde) {
                self.mid_boundary = cursor.peek_next().clone_pointer();
            } else if optr(&self.old_boundary) == ptr(&cde) {
                self.old_boundary = cursor.peek_next().clone_pointer();
            }
            cursor.remove();
        };
        self.hash.remove(&cde.key);
        cde
    }
}

fn switch_chain<K, V, const C: usize>(
    old_count: usize,
    new_count: usize,
    chains: &mut [LinkedList<ListChain<K, V>>; C],
    cde: &UnsafeRef<FbrEntry<K, V>>,
) {
    if old_count < C {
        unsafe { chains[old_count].cursor_mut_from_ptr(cde.as_ref()) }.remove();
    }
    if new_count < C {
        chains[new_count].push_front(cde.clone());
    }
}

fn move_boundaries<K, V>(
    from_region: Region,
    len: usize,
    mid: usize,
    old: usize,
    lru: &LinkedList<ListLru<K, V>>,
    mid_boundary: &mut Option<UnsafeRef<FbrEntry<K, V>>>,
    old_boundary: &mut Option<UnsafeRef<FbrEntry<K, V>>>,
) {
    if from_region > Region::New {
        if let Some(mid) = mid_boundary {
            let cursor = unsafe { lru.cursor_from_ptr(mid.as_ref()) };
            let ptr = cursor.peek_prev().clone_pointer().unwrap();
            FbrEntry::region(&ptr, Region::Middle);
            *mid_boundary = Some(ptr);
        } else if len == mid + 1 {
            let ptr = lru.back().clone_pointer().unwrap();
            FbrEntry::region(&ptr, Region::Middle);
            *mid_boundary = Some(ptr);
        }
    }
    if from_region > Region::Middle {
        if let Some(old) = old_boundary {
            let cursor = unsafe { lru.cursor_from_ptr(old.as_ref()) };
            let ptr = cursor.peek_prev().clone_pointer().unwrap();
            FbrEntry::region(&ptr, Region::Old);
            *old_boundary = Some(ptr);
        } else if len == old + 1 {
            let ptr = lru.back().clone_pointer().unwrap();
            FbrEntry::region(&ptr, Region::Old);
            *old_boundary = Some(ptr);
        }
    }
}

fn ptr<T>(p: &UnsafeRef<T>) -> *const T {
    UnsafeRef::into_raw(p.clone())
}

fn optr<T>(p: &Option<UnsafeRef<T>>) -> *const T {
    p.as_ref().map(ptr).unwrap_or(null())
}
