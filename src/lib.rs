#[cfg(test)]
extern crate fnv;
#[cfg(test)]
extern crate rand;

use std::mem::{self, ManuallyDrop};
use std::ptr;
use std::hash::{BuildHasher, Hash, Hasher};
use std::fmt;

const BLOCK_SIZE: usize = 16;

const JUMP_DISTANCES: [usize; 126] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,

    21, 28, 36, 45, 55, 66, 78, 91, 105, 120, 136, 153, 171, 190, 210, 231,
    253, 276, 300, 325, 351, 378, 406, 435, 465, 496, 528, 561, 595, 630,
    666, 703, 741, 780, 820, 861, 903, 946, 990, 1035, 1081, 1128, 1176,
    1225, 1275, 1326, 1378, 1431, 1485, 1540, 1596, 1653, 1711, 1770, 1830,
    1891, 1953, 2016, 2080, 2145, 2211, 2278, 2346, 2415, 2485, 2556,

    3741, 8385, 18915, 42486, 95703, 215496, 485605, 1091503, 2456436,
    5529475, 12437578, 27986421, 62972253, 141700195, 318819126, 717314626,
    1614000520, 3631437253, 8170829695, 18384318876, 41364501751,
    93070021080, 209407709220, 471167588430, 1060127437995, 2385287281530,
    5366895564381, 12075513791265, 27169907873235, 61132301007778,
    137547673121001, 309482258302503, 696335090510256, 1566753939653640,
    3525196427195653, 7931691866727775, 17846306747368716,
    40154190394120111, 90346928493040500, 203280588949935750,
    457381324898247375, 1029107980662394500, 2315492957028380766,
    5209859150892887590,
];

struct Metadata(u8);
impl Metadata {
    fn is_empty(&self) -> bool {
        self.0 == 0b11111111
    }
    fn is_storage(&self) -> bool {
        self.0 & 0b10000000 == 0b10000000
    }
    fn jump_length(&self) -> u8 {
        assert!(!self.is_empty());
        self.0 & 0b01111111
    }
    fn set_last(&mut self, storage: bool) {
        self.set_storage(storage);
        self.set_jump(0);
    }
    fn set_storage(&mut self, storage: bool) {
        if storage {
            self.0 |= 0b10000000;
        } else {
            self.0 &= 0b01111111;
        }
    }
    fn set_empty(&mut self) {
        self.0 = 0b11111111;
    }
    fn set_jump(&mut self, jump: u8) {
        assert!(jump & 0b10000000 == 0b00000000);
        self.0 &= 0b10000000;
        self.0 |= jump;
    }
}

impl fmt::Debug for Metadata {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if self.is_empty() {
            fmt.write_str("~--")
        } else if self.is_storage() {
            write!(fmt, "<{:02x}", self.jump_length())
        } else {
            write!(fmt, "|{:02x}", self.jump_length())
        }
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Metadata(0b11111111)
    }
}

#[derive(Default)]
struct Metadatum([Metadata; BLOCK_SIZE]);

struct Entry<K, V> {
    key: K,
    value: V
}

impl<K, V> Entry<K, V> {
    fn new(key: K, value: V) -> Self {
        Entry {
            key,
            value
        }
    }
}

struct Datum<K, V>([Entry<K, V>; BLOCK_SIZE]);

struct Cell<K, V> {
    meta: Metadatum,
    data: ManuallyDrop<Datum<K, V>>
}

impl<K, V> Drop for Cell<K, V> {
    fn drop(&mut self) {
        unsafe {
            for slot in 0..BLOCK_SIZE {
                if !self.meta.0[slot].is_empty() {
                    let datum_ptr = self.data.0.as_mut().as_mut_ptr();
                    datum_ptr.offset(slot as isize).drop_in_place();
                }
            }
        }
    }
}

pub struct HashMap<K, V, H> {
    ptr: *mut Cell<K, V>,
    size: usize,
    capacity: usize,
    hasher: H
}

impl<K, V, H> Drop for HashMap<K, V, H> {
    fn drop(&mut self) {
        unsafe {
            drop(Vec::from_raw_parts(self.ptr, 0, self.capacity));
        }
    }
}

impl<'a, K: 'a, V: 'a, H: 'a> IntoIterator for &'a HashMap<K, V, H>
    where K: Hash + PartialEq,
          H: BuildHasher
{
    type IntoIter = Iter<'a, K, V, H>;
    type Item = (&'a K, &'a V);
    fn into_iter(self) -> Self::IntoIter {
        Iter(self, 0, 0)
    }
}

pub struct Iter<'a, K: 'a, V: 'a, H: 'a>(&'a HashMap<K, V, H>, usize, usize);

impl<'a, K: 'a, V: 'a, H: 'a> Iterator for Iter<'a, K, V, H>
    where K: Hash + PartialEq,
          H: BuildHasher
{
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let (cell, slot) = (&mut self.1, &mut self.2);
            while *cell < self.0.capacity {
                let cur_cell = self.0.ptr.offset(*cell as isize);
                let cur_slot = *slot;
                *slot += 1;
                if *slot >= BLOCK_SIZE {
                    *slot = 0;
                    *cell += 1;
                }
                if !(*cur_cell).meta.0[cur_slot].is_empty() {
                    let datum_ptr = (*cur_cell).data.0.as_ref().as_ptr();
                    let entry = datum_ptr.offset(cur_slot as isize);
                    return Some((&(*entry).key, &(*entry).value));
                }
            }
            None
        }
    }
}

pub struct IterMut<'a, K: 'a, V: 'a, H: 'a>(&'a mut HashMap<K, V, H>, usize, usize);

impl<'a, K: 'a, V: 'a, H: 'a> Iterator for IterMut<'a, K, V, H>
    where K: Hash + PartialEq,
          H: BuildHasher
{
    type Item = (&'a mut K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let (cell, slot) = (&mut self.1, &mut self.2);
            while *cell < self.0.capacity {
                let cur_cell = self.0.ptr.offset(*cell as isize);
                let cur_slot = *slot;
                *slot += 1;
                if *slot >= BLOCK_SIZE {
                    *slot = 0;
                    *cell += 1;
                }
                if !(*cur_cell).meta.0[cur_slot].is_empty() {
                    let datum_ptr = (*cur_cell).data.0.as_mut().as_mut_ptr();
                    let entry = datum_ptr.offset(cur_slot as isize);
                    return Some((&mut (*entry).key, &mut (*entry).value));
                }
            }
            None
        }
    }
}

impl<K, V, H> Default for HashMap<K, V, H>
    where K: Hash + PartialEq,
          H: BuildHasher + Default
{
    fn default() -> Self {
        Self::with_hasher(H::default())
    }
}

impl<K, V, H> HashMap<K, V, H>
    where K: Hash + PartialEq,
          H: BuildHasher
{
    pub fn with_hasher(hasher: H) -> Self {
        HashMap {
            ptr: allocate(1),
            size: 0,
            capacity: 1,
            hasher
        }
    }

    pub fn with_capacity(capacity: usize, hasher: H) -> Self {
        let capacity = ((capacity as f32 / BLOCK_SIZE as f32).ceil() as usize).next_power_of_two();
        HashMap {
            ptr: allocate(capacity),
            size: 0,
            capacity,
            hasher
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        if self.size as f32 / (BLOCK_SIZE * self.capacity) as f32 > 0.872 {
            self.reallocate();
        }
        let mut hash = self.hash(&key) as usize; // NOTE: Possible panic
        unsafe {
            let mut cur_meta = ptr::null_mut();
            let mut data_ptr = ptr::null_mut();
            self.mut_data(hash, &mut cur_meta, &mut data_ptr);

            if (*cur_meta).is_empty() {
                (*cur_meta).set_last(false);
                ptr::write(data_ptr, Entry::new(key, value));
                self.size += 1;
                return None;
            } else if (*cur_meta).is_storage() {
                let (mut prev_meta, mut relocate_meta) = (ptr::null_mut(), ptr::null_mut());

                let prev_hash = self.find_previous(hash, data_ptr);
                self.mut_data(prev_hash, &mut prev_meta, &mut ptr::null_mut());

                let mut first_jump = (*prev_meta).jump_length();
                let mut to_be_moved = ptr::read(data_ptr);
                let mut to_be_moved_place = hash;
                let mut jump_to_next_to_be_moved = (*cur_meta).jump_length();

                let mut cur_hash = prev_hash;

                loop {
                    if let Some((relocate_data_ptr, jumps)) = self.find_empty(cur_hash, first_jump, &mut relocate_meta) {
                        first_jump = 1;
                        (*relocate_meta).set_last(true);
                        ptr::write(relocate_data_ptr, to_be_moved);
                        (*prev_meta).set_jump(jumps);
                        mem::swap(&mut prev_meta, &mut relocate_meta);

                        (*cur_meta).set_empty();

                        if jump_to_next_to_be_moved == 0 {
                            break;
                        }

                        cur_hash += JUMP_DISTANCES[jumps as usize];
                        to_be_moved_place += JUMP_DISTANCES[jump_to_next_to_be_moved as usize];
                        self.mut_data(to_be_moved_place, &mut cur_meta, &mut data_ptr);
                        to_be_moved = ptr::read(data_ptr);
                        jump_to_next_to_be_moved = (*cur_meta).jump_length();
                    } else {
                        self.reallocate();
                        self.insert(to_be_moved.key, to_be_moved.value);
                        return self.insert(key, value);
                    }
                }
                self.mut_data(hash, &mut cur_meta, &mut data_ptr);
                (*cur_meta).set_last(false);
                ptr::write(data_ptr, Entry::new(key, value));
                self.size += 1;
                return None;
            }
            loop {
                debug_assert!(!(*cur_meta).is_empty());
                let data = &mut *data_ptr;
                if data.key == key { // NOTE: Possible panic
                    let value_ptr = (&mut data.value) as *mut _;
                    return Some((key, ptr::replace(value_ptr, value)));
                }
                let jump = (*cur_meta).jump_length();
                if jump == 0 {
                    let prev_meta = cur_meta;
                    let mut cur_meta = ptr::null_mut();
                    return if let Some((data_ptr, jumps)) = self.find_empty(hash, 1, &mut cur_meta) {
                        (*cur_meta).set_last(true);
                        (*prev_meta).set_jump(jumps);
                        ptr::write(data_ptr, Entry::new(key, value));
                        self.size += 1;
                        None
                    } else {
                        self.reallocate();
                        self.insert(key, value)
                    };
                }
                hash += JUMP_DISTANCES[jump as usize];

                self.mut_data(hash, &mut cur_meta, &mut data_ptr);
            }
        }
    }

    unsafe fn find_previous(&self, target_hash: usize, data_ptr: *const Entry<K, V>) -> usize {
        let mut their_hash = self.hash(&(*data_ptr).key) as usize; // NOTE: Possible panic
        let mut prev_hash = 0;
        let mut before_meta = ptr::null();
        while split_hash(their_hash, self.capacity) != split_hash(target_hash, self.capacity) {
            prev_hash = their_hash;
            self.get_data(their_hash, &mut before_meta, &mut ptr::null());
            debug_assert!((*before_meta).jump_length() != 0);
            their_hash += JUMP_DISTANCES[(*before_meta).jump_length() as usize];
        }
        return prev_hash;
    }

    unsafe fn find_empty<'b>(&mut self, hash: usize, start: u8, meta: &'b mut *mut Metadata) -> Option<(*mut Entry<K, V>, u8)> {
        let mut data_ptr = ptr::null_mut();
        for jumps in (start as usize)..JUMP_DISTANCES.len() {
            let new_hash = hash.wrapping_add(JUMP_DISTANCES[jumps]);

            self.mut_data(new_hash, meta, &mut data_ptr);

            if (**meta).is_empty() {
                return Some((data_ptr, jumps as u8));
            }
        }
        None
    }

    pub fn remove(&mut self, key: &K) -> Option<(K, V)> {
        let mut hash = self.hash(&key) as usize; // NOTE: Possible panic
        unsafe {
            let mut cur_meta = ptr::null_mut();
            let mut data_ptr = ptr::null_mut();
            self.mut_data(hash, &mut cur_meta, &mut data_ptr);

            if (*cur_meta).is_storage() {
                return None;
            }
            let mut prev_meta = cur_meta;
            loop {
                if &(*data_ptr).key == key { // NOTE: Possible panic
                    let data = ptr::read(data_ptr);
                    let mut prev_ptr;
                    loop {
                        let jump = (*cur_meta).jump_length();
                        if jump == 0 {
                            (*prev_meta).set_jump(0);
                            (*cur_meta).set_empty();
                            break;
                        }
                        prev_ptr = data_ptr;
                        prev_meta = cur_meta;
                        hash += JUMP_DISTANCES[jump as usize];
                        self.mut_data(hash, &mut cur_meta, &mut data_ptr);
                        ptr::write(prev_ptr, ptr::read(data_ptr));
                    }
                    return Some((data.key, data.value));
                }
                let jump = (*cur_meta).jump_length();
                if jump == 0 {
                    return None;
                }
                hash += JUMP_DISTANCES[jump as usize];
                prev_meta = cur_meta;
                self.mut_data(hash, &mut cur_meta, &mut data_ptr);
            }
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let mut hash = self.hash(&key) as usize; // NOTE: Possible panic
        unsafe {
            let mut cur_meta = ptr::null();
            let mut data_ptr = ptr::null();
            self.get_data(hash, &mut cur_meta, &mut data_ptr);

            if (*cur_meta).is_storage() {
                return None;
            }
            loop {
                let data = &*data_ptr;
                if &data.key == key { // NOTE: Possible panic
                    return Some(&data.value);
                }
                let jump = (*cur_meta).jump_length();
                if jump == 0 {
                    return None;
                }
                hash += JUMP_DISTANCES[jump as usize];
                self.get_data(hash, &mut cur_meta, &mut data_ptr);
            }
        }
    }

    // TODO: Abstract over mutability. Needs HKT/GAT
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let mut hash = self.hash(&key) as usize; // NOTE: Possible panic
        unsafe {
            let mut cur_meta = ptr::null_mut();
            let mut data_ptr = ptr::null_mut();
            self.mut_data(hash, &mut cur_meta, &mut data_ptr);

            if (*cur_meta).is_storage() {
                return None;
            }
            loop {
                let data = &mut *data_ptr;
                if &data.key == key { // NOTE: Possible panic
                    return Some(&mut data.value);
                }
                let jump = (*cur_meta).jump_length();
                if jump == 0 {
                    return None;
                }
                hash += JUMP_DISTANCES[jump as usize];
                self.mut_data(hash, &mut cur_meta, &mut data_ptr);
            }
        }
    }

    fn reallocate(&mut self) {
        let old_capacity = self.capacity;
        let new_capacity = 2 * self.capacity;
        self.capacity = new_capacity;
        self.size = 0;
        let old_ptr = mem::replace(&mut self.ptr, allocate(new_capacity));

        unsafe {
            for cell in 0..old_capacity {
                let cur_cell = old_ptr.offset(cell as isize);
                for slot in 0..BLOCK_SIZE {
                    if !&(*cur_cell).meta.0[slot].is_empty() {
                        (*cur_cell).meta.0[slot].set_empty();
                        let datum_ptr = (*cur_cell).data.0.as_ref().as_ptr();
                        let data_ptr = datum_ptr.offset(slot as isize);
                        let entry = ptr::read(data_ptr);
                        self.insert(entry.key, entry.value);
                    }
                }
            }
            drop(Vec::from_raw_parts(old_ptr, 0, old_capacity))
        }
    }

    fn hash(&self, key: &K) -> u64 {
        let mut state = self.hasher.build_hasher();
        key.hash(&mut state);
        state.finish()
    }

    fn get_data(&self, hash: usize, cur_meta: &mut *const Metadata, data_ptr: &mut *const Entry<K, V>) {
        unsafe {
            let (cell, slot) = split_hash(hash, self.capacity);
            let cur_cell = self.ptr.offset(cell);

            let meta_ptr = (*cur_cell).meta.0.as_ref().as_ptr();
            *cur_meta = meta_ptr.offset(slot as isize);
            
            let datum_ptr = (*cur_cell).data.0.as_ref().as_ptr();
            *data_ptr = datum_ptr.offset(slot as isize);
        }
    }
    
    // TODO: Abstract over mutability. Needs HKT/GAT
    fn mut_data(&mut self, hash: usize, cur_meta: &mut *mut Metadata, data_ptr: &mut *mut Entry<K, V>) {
        unsafe {
            let (cell, slot) = split_hash(hash, self.capacity);
            let cur_cell = self.ptr.offset(cell);

            let meta_ptr = (*cur_cell).meta.0.as_mut().as_mut_ptr();
            *cur_meta = meta_ptr.offset(slot as isize);

            let datum_ptr = (*cur_cell).data.0.as_mut().as_mut_ptr();
            *data_ptr = datum_ptr.offset(slot as isize);
        }
    }

    fn debug(&self) {
        unsafe {
            let numbers = (self.capacity as f32).log(10.) as usize + 1;
            for cell in 0..self.capacity {
                let cur_cell = self.ptr.offset(cell as isize);
                print!("{:w$} ", cell, w = numbers);
                for slot in 0..BLOCK_SIZE {
                    print!("{:?}", (*cur_cell).meta.0[slot]);
                }
                print!(" ");
                if cell % 2 == 1 {
                    println!();
                }
            }
            if self.capacity % 2 == 1 {
                println!();
            }
        }
    }
}

fn allocate<K, V>(capacity: usize) -> *mut Cell<K, V> {
    // TODO: This is inefficent
    let mut data = Vec::with_capacity(capacity);
    for _ in 0..capacity {
        data.push(Cell {
            meta: Metadatum::default(),
            data: unsafe { mem::uninitialized() },
        });
    }
    let ptr = data.as_mut_ptr();
    mem::forget(data);
    ptr
}

fn split_hash(hash: usize, capacity: usize) -> (isize, usize) {
    (
        ((hash / BLOCK_SIZE) & (capacity - 1)) as isize,
        hash & (BLOCK_SIZE - 1)
    )
}

#[test]
fn adding_one_works() {
    let max = 10000;
    for n in 0..max {
        let mut map = HashMap::with_hasher(::fnv::FnvBuildHasher::default());
        map.insert(n, n);
        assert_eq!(Some(&n), map.get(&n));
    }
}

#[test]
fn adding_multiple_works() {
    let max = 10000;
    let mut map = HashMap::with_hasher(::fnv::FnvBuildHasher::default());
    for n in 0..max {
        map.insert(n, n);
    }
    for n in 0..max {
        let val = map.get(&n);
        if Some(&n) != val {
            map.debug();
            panic!("Getting {} at {:?} failed. Was: {:?}", n, split_hash(map.hash(&n) as usize, map.capacity), val);
        }
    }
}

#[test]
fn adding_random_works() {
    use rand::rngs::SmallRng;
    use rand::distributions::Standard;
    use rand::{SeedableRng, Rng};
    let max = 10000;
    let mut map = HashMap::<u64, u64, _>::with_hasher(::fnv::FnvBuildHasher::default());
    let mut rng = SmallRng::from_seed([0, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 3, 5]);
    let mut rng2 = rng.clone();
    for n in rng.sample_iter(&Standard).take(max) {
        map.insert(n, n);
    }
    for n in rng2.sample_iter(&Standard).take(max) {
        let val = map.get(&n);
        if Some(&n) != val {
            map.debug();
            panic!("Getting {} at {:?} failed. Was: {:?}", n, split_hash(map.hash(&n) as usize, map.capacity), val);
        }
    }
}

#[test]
fn removing_works() {
    let max = 10000;
    let mut map = HashMap::with_hasher(::fnv::FnvBuildHasher::default());
    for n in 0..max {
        map.insert(n, n);
    }
    for n in 0..max {
        let val = map.remove(&n);
        if Some((n, n)) != val {
            map.debug();
            panic!("Removing {} at {:?} failed. Was: {:?}", n, split_hash(map.hash(&n) as usize, map.capacity), val);
        }
    }
    for n in 0..max {
        assert!(map.get(&n).is_none());
    }
}

#[test]
fn iterator_works() {
    use std::collections::HashMap as HMap;
    let max = 10000;
    let mut map = HashMap::with_hasher(::fnv::FnvBuildHasher::default());
    let mut added = HMap::new();
    for n in 0..max {
        map.insert(n, n);
        added.insert(n, n);
    }

    for (k, v) in &map {
        assert_eq!(Some(v), added.remove(k).as_ref());
    }
    assert!(added.is_empty());
}