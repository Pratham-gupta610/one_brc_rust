//NOTE: After multithreading  --> 3.2 sec approx execution time with 18-22 threads

use std::{
    ffi::{c_int, c_void},
    fs::File,
    io::Write,
    os::fd::AsRawFd,
    //NOTE: The above   os::fd::AsRawFd, works only on unix environment for windows it will throw error
    time::Instant,
};

const SLOTS: usize = 1 << 12;
const MASK: usize = SLOTS - 1;

struct Entry {
    key: [u8; 100],
    klen: u8,
    min: i32,
    max: i32,
    sum: i64,
    count: u32,
}

impl Entry {
    const fn zero() -> Self {
        Self {
            key: [0; 100],
            klen: 0,
            min: i32::MAX,
            max: i32::MIN,
            sum: 0,
            count: 0,
        }
    }

    #[inline(always)]
    fn name(&self) -> &[u8] {
        &self.key[..self.klen as usize]
    }
}

struct Table {
    entries: Vec<Entry>,
    hashes: Vec<u32>,
}

impl Table {
    fn new() -> Self {
        let mut entries = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            entries.push(Entry::zero());
        }

        Self {
            entries,
            hashes: vec![0; SLOTS],
        }
    }

    #[inline(always)]
    fn get_or_insert(&mut self, name: &[u8], h: u32) -> &mut Entry {
        let h = if h == 0 { 1 } else { h };

        let hashes = self.hashes.as_mut_ptr();
        let entries = self.entries.as_mut_ptr();

        let mut i = h as usize & MASK;

        unsafe {
            loop {
                let slot_h = *hashes.add(i);

                if slot_h == 0 {
                    *hashes.add(i) = h;

                    let e = &mut *entries.add(i);
                    e.klen = name.len() as u8;

                    std::ptr::copy_nonoverlapping(name.as_ptr(), e.key.as_mut_ptr(), name.len());

                    e.min = i32::MAX;
                    e.max = i32::MIN;
                    e.sum = 0;
                    e.count = 0;

                    return e;
                }

                if slot_h == h {
                    let e = &mut *entries.add(i);

                    if e.klen as usize == name.len()
                        && libc::memcmp(
                            e.key.as_ptr() as *const c_void,
                            name.as_ptr() as *const c_void,
                            name.len(),
                        ) == 0
                    {
                        return e;
                    }
                }

                i = (i + 1) & MASK;
            }
        }
    }

    #[inline(always)]
    fn merge_entry(&mut self, e: &Entry, h: u32) {
        let dst = self.get_or_insert(e.name(), h);

        if e.min < dst.min {
            dst.min = e.min;
        }

        if e.max > dst.max {
            dst.max = e.max;
        }

        dst.sum += e.sum;
        dst.count += e.count;
    }
}

#[inline(always)]
fn find_byte(data: &[u8], byte: u8) -> usize {
    let p = unsafe { libc::memchr(data.as_ptr() as *const c_void, byte as c_int, data.len()) };

    if p.is_null() {
        data.len()
    } else {
        unsafe { (p as *const u8).offset_from(data.as_ptr()) as usize }
    }
}

fn process(data: &[u8]) -> Table {
    let mut table = Table::new();
    let mut pos = 0usize;
    let len = data.len();

    while pos < len {
        if unsafe { *data.get_unchecked(pos) } == b'\n' {
            pos += 1;
            continue;
        }

        let name_start = pos;
        let mut h = 2166136261u32;

        unsafe {
            loop {
                let b = *data.get_unchecked(pos);

                if b == b';' {
                    break;
                }

                h ^= b as u32;
                h = h.wrapping_mul(16777619);
                pos += 1;
            }
        }

        let name = &data[name_start..pos];
        pos += 1;

        let t: i32;

        unsafe {
            let b0 = *data.get_unchecked(pos);

            if b0 == b'-' {
                let b2 = *data.get_unchecked(pos + 2);

                if b2 == b'.' {
                    t = -(((*data.get_unchecked(pos + 1) - b'0') as i32) * 10
                        + (*data.get_unchecked(pos + 3) - b'0') as i32);
                    pos += 4;
                } else {
                    t = -(((*data.get_unchecked(pos + 1) - b'0') as i32) * 100
                        + ((*data.get_unchecked(pos + 2) - b'0') as i32) * 10
                        + (*data.get_unchecked(pos + 4) - b'0') as i32);
                    pos += 5;
                }
            } else {
                let b1 = *data.get_unchecked(pos + 1);

                if b1 == b'.' {
                    t = ((*data.get_unchecked(pos) - b'0') as i32) * 10
                        + (*data.get_unchecked(pos + 2) - b'0') as i32;
                    pos += 3;
                } else {
                    t = ((*data.get_unchecked(pos) - b'0') as i32) * 100
                        + ((*data.get_unchecked(pos + 1) - b'0') as i32) * 10
                        + (*data.get_unchecked(pos + 3) - b'0') as i32;
                    pos += 4;
                }
            }

            if pos < len && *data.get_unchecked(pos) == b'\n' {
                pos += 1;
            }
        }

        let e = table.get_or_insert(name, h);

        if t < e.min {
            e.min = t;
        }

        if t > e.max {
            e.max = t;
        }

        e.sum += t as i64;
        e.count += 1;
    }

    table
}

fn mmap(f: &File) -> &'static [u8] {
    let len = f.metadata().unwrap().len() as usize;

    //This is showing errors because `libc::mmap`, `MAP_SHARED`, `madvise`,
    // `MADV_SEQUENTIAL`, `MADV_HUGEPAGE`, and `as_raw_fd()` are Unix-only APIs.
    // If you're editing this in VS Code on Windows (even if the project is inside WSL),
    // rust-analyzer is probably using the Windows target (`x86_64-pc-windows-msvc`).
    // On Windows:
    // - `AsRawFd` does not exist (Windows uses `AsRawHandle`)
    // - `mmap` is not provided like Linux `mmap`
    // - `madvise` flags don't exist
    //
    // So the code itself is fine for Linux/WSL, but the analyzer marks it red
    // because it thinks you're compiling for Windows.
    //
    // Fix:
    // 1. Open the project from WSL using:
    //    code .
    //    (inside Ubuntu/WSL terminal)
    //
    // 2. Or set rust-analyzer target manually in VSCode:
    //    "rust-analyzer.cargo.target": "x86_64-unknown-linux-gnu"
    //
    // Then the red squiggles disappear.
    // NOTE:
    unsafe {
        let ptr = libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            f.as_raw_fd(),
            0,
        );

        assert_ne!(
            ptr,
            libc::MAP_FAILED,
            "mmap failed: {:?}",
            std::io::Error::last_os_error()
        );

        libc::madvise(ptr, len, libc::MADV_SEQUENTIAL);
        libc::madvise(ptr, len, libc::MADV_HUGEPAGE);

        std::slice::from_raw_parts(ptr as *const u8, len)
    }
}

fn next_newline(data: &[u8], pos: usize) -> usize {
    if pos >= data.len() {
        return data.len();
    }

    let off = find_byte(&data[pos..], b'\n');
    (pos + off + 1).min(data.len())
}

fn main() {
    let t0 = Instant::now();

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/measurements.txt".to_string());

    let f = File::open(&path).unwrap();
    let file_size = f.metadata().unwrap().len();
    let data = mmap(&f);

    let nthreads = std::env::var("THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().unwrap().get());

    let mut merged = Table::new();

    std::thread::scope(|scope| {
        let chunk = data.len() / nthreads;
        let mut at = 0usize;
        let mut handles = Vec::with_capacity(nthreads);

        for i in 0..nthreads {
            let start = at;

            at = if i == nthreads - 1 {
                data.len()
            } else {
                next_newline(data, at + chunk)
            };

            let sl = &data[start..at];

            handles.push(scope.spawn(move || process(sl)));
        }

        for handle in handles {
            let table = handle.join().unwrap();

            for (e, &h) in table.entries.iter().zip(table.hashes.iter()) {
                if h != 0 {
                    merged.merge_entry(e, h);
                }
            }
        }
    });

    let mut final_entries: Vec<(&Entry, u32)> = merged
        .entries
        .iter()
        .zip(merged.hashes.iter())
        .filter_map(|(e, &h)| if h != 0 { Some((e, h)) } else { None })
        .collect();

    final_entries.sort_unstable_by(|a, b| a.0.name().cmp(b.0.name()));

    let mut out = std::io::BufWriter::with_capacity(1 << 17, std::io::stdout().lock());

    write!(out, "{{").unwrap();

    for (idx, (e, _)) in final_entries.iter().enumerate() {
        let name = unsafe { std::str::from_utf8_unchecked(e.name()) };

        write!(
            out,
            "{name}={:.1}/{:.1}/{:.1}",
            e.min as f64 / 10.0,
            e.sum as f64 / e.count as f64 / 10.0,
            e.max as f64 / 10.0,
        )
        .unwrap();

        if idx + 1 != final_entries.len() {
            write!(out, ", ").unwrap();
        }
    }

    writeln!(out, "}}").unwrap();

    let e = t0.elapsed();
    let gb = file_size as f64 / 1_073_741_824.0;
    let throughput = gb / e.as_secs_f64();

    eprintln!("\n\x1b[1;36m═══════════════════════════════════════════════\x1b[0m");
    eprintln!("\x1b[1;35m               🚀 1BRC RUN STATS 🚀\x1b[0m");
    eprintln!("\x1b[1;36m═══════════════════════════════════════════════\x1b[0m");
    eprintln!(
        "\x1b[1;33m📂 File Size   :\x1b[0m \x1b[1;37m{:.2} GB\x1b[0m",
        gb
    );
    eprintln!(
        "\x1b[1;34m🧵 Threads     :\x1b[0m \x1b[1;37m{}\x1b[0m",
        nthreads
    );
    eprintln!(
        "\x1b[1;32m⚡ Time Taken  :\x1b[0m \x1b[1;92m{:.3?}\x1b[0m",
        e
    );
    eprintln!(
        "\x1b[1;31m🔥 Throughput :\x1b[0m \x1b[1;91m{:.2} GB/s\x1b[0m",
        throughput
    );
    eprintln!("\x1b[1;36m═══════════════════════════════════════════════\x1b[0m\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_manual_cases() {
        let data = b"X;0.0\nX;9.2\nX;-9.2\nX;98.2\nX;-98.2\n";
        let table = process(data);
        let mut found = false;

        for (e, &h) in table.entries.iter().zip(table.hashes.iter()) {
            if h != 0 && e.name() == b"X" {
                found = true;
                assert_eq!(e.min, -982);
                assert_eq!(e.max, 982);
                assert_eq!(e.sum, 0);
                assert_eq!(e.count, 5);
            }
        }

        assert!(found);
    }
}

// #![feature(portable_simd)]
// #![feature(slice_split_once)]
/*
//NOTE: before multithreading one --> 22 sec approx execution time
use memmap2::Mmap;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    hash::{BuildHasher, Hasher},

    io::Write,
    os::fd::AsRawFd,
    //NOTE: The above   os::fd::AsRawFd, works only on unix environment for windows it will throw error
    // simd::{cmp::SimdPartialEq, u8x64},
    time::Instant,
};
pub struct FastHasherBuilder;

pub struct FastHasher {
    len: u64,
    hash: u64,
}

impl BuildHasher for FastHasherBuilder {
    type Hasher = FastHasher;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        FastHasher { len: 0, hash: 0 }
    }
}

impl Hasher for FastHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.hash
    }

    #[inline(always)]
    fn write_usize(&mut self, i: usize) {
        self.len = i as u64;
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        unsafe {
            let l = bytes.len();
            let len = if self.len != 0 { self.len } else { l as u64 };
            let ptr = bytes.as_ptr();

            let (first, last, mid) = if l >= 16 {
                (
                    (ptr as *const u64).read_unaligned(),
                    (ptr.add(l - 8) as *const u64).read_unaligned(),
                    (ptr.add(l / 2 - 4) as *const u64).read_unaligned(),
                )
            } else if l >= 8 {
                let first = (ptr as *const u64).read_unaligned();
                let last = (ptr.add(l - 8) as *const u64).read_unaligned();
                (first, last, first)
            } else {
                let small = read_small(ptr, l);
                (small, small, small)
            };

            let mut h = first;
            h ^= last.rotate_left(23);
            h ^= mid.rotate_left(41);
            h ^= len.wrapping_mul(0x9E37_79B1_85EB_CA87);

            h ^= h >> 33;
            h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
            h ^= h >> 33;
            h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
            h ^= h >> 33;

            self.hash = h | (h == 0) as u64;
        }
    }
}

#[inline(always)]
unsafe fn read_small(ptr: *const u8, len: usize) -> u64 {
    match len {
        0 => 0,
        1 => *ptr as u64,
        2 => (*ptr as u64) | ((*ptr.add(1) as u64) << 8),
        3 => (*ptr as u64) | ((*ptr.add(1) as u64) << 8) | ((*ptr.add(2) as u64) << 16),
        4 => {
            (*ptr as u64)
                | ((*ptr.add(1) as u64) << 8)
                | ((*ptr.add(2) as u64) << 16)
                | ((*ptr.add(3) as u64) << 24)
        }
        5 => {
            (*ptr as u64)
                | ((*ptr.add(1) as u64) << 8)
                | ((*ptr.add(2) as u64) << 16)
                | ((*ptr.add(3) as u64) << 24)
                | ((*ptr.add(4) as u64) << 32)
        }
        6 => {
            (*ptr as u64)
                | ((*ptr.add(1) as u64) << 8)
                | ((*ptr.add(2) as u64) << 16)
                | ((*ptr.add(3) as u64) << 24)
                | ((*ptr.add(4) as u64) << 32)
                | ((*ptr.add(5) as u64) << 40)
        }
        7 => {
            (*ptr as u64)
                | ((*ptr.add(1) as u64) << 8)
                | ((*ptr.add(2) as u64) << 16)
                | ((*ptr.add(3) as u64) << 24)
                | ((*ptr.add(4) as u64) << 32)
                | ((*ptr.add(5) as u64) << 40)
                | ((*ptr.add(6) as u64) << 48)
        }
        _ => std::hint::unreachable_unchecked(),
    }
}
// struct FastHasherBuilder;
// struct FastHasher(u64);

// impl BuildHasher for FastHasherBuilder {
//     type Hasher = FastHasher;

//     fn build_hasher(&self) -> Self::Hasher {
//         FastHasher(0xcbf29ce484222325)
//     }
// }

// impl Hasher for FastHasher {
//     #[inline(always)]
//     fn finish(&self) -> u64 {
//         self.0
//     }
//     #[inline(always)]
//     fn write(&mut self, bytes: &[u8]) {
//         let (chunks, remainder) = bytes.as_chunks::<8>();
//         let mut last = [1u8; 8];
//         (last[..remainder.len()]).copy_from_slice(remainder);
//         for &chunk in chunks.iter().chain(std::iter::once(&last)) {
//             let mixed = self.0 as u128 * (u64::from_ne_bytes(chunk) as u128);
//             self.0 = (mixed >> 64) as u64 ^ mixed as u64;
//         }
//     }
// }
fn main() {
    let start = Instant::now();
    let path: &str = "data/measurements.txt";
    let file_size: u64 = std::fs::metadata(path).unwrap().len();

    let f: File = File::open(path).unwrap();
    // let f: BufReader<File> = BufReader::new(f);
    let map: Mmap = mmap(&f);
    // NOTE: maybe make the key &[u8], but measure since we're breaking MADV_SEQUENTIAL

    let mut stats = HashMap::<Vec<u8>, (i16, i64, usize, i16), _>::with_capacity_and_hasher(
        600,
        FastHasherBuilder,
    );

    let mut at = 0;

    let mut rows = 0usize;

    // for line in map.split(|c| *c == b'\n')
    loop {
        let line = next_line(&map, &mut at);
        if line.is_empty() {
            break;
        }

        rows += 1;

        let (station, temperature) = split_semi(line);

        // SAFETY: the README promised

        let t = parse_temperature(temperature);
        let stats = match stats.get_mut(station) {
            Some(stats) => stats,
            None => stats
                .entry(station.to_vec())
                .or_insert((i16::MAX, 0, 0, i16::MIN)),
        };
        stats.0 = stats.0.min(t);
        stats.1 += t as i64;
        stats.2 += 1;
        stats.3 = stats.3.max(t);
        // stats.0 = stats.0.min(t);
        // stats.1 += temperature;
        // stats.2 += 1;
        // stats.3 = stats.3.max(temperature);
    }

    let station_count = stats.len();

    print!("{{");

    let stats = BTreeMap::from_iter(
        stats
            .into_iter()
            .map(|(k, v)| (unsafe { String::from_utf8_unchecked(k) }, v)),
    );

    let mut stats = stats.into_iter().peekable();

    while let Some((station, (min, sum, count, max))) = stats.next() {
        print!(
            "{station}={:.1}/{:.1}/{:.1}",
            min as f64 / 10.0,
            sum as f64 / count as f64 / 10.0,
            max as f64 / 10.0
        );

        if stats.peek().is_some() {
            print!(", ");
        }
    }

    print!("}}");

    std::io::stdout().flush().unwrap();

    let elapsed = start.elapsed();
    let seconds = elapsed.as_secs_f64();

    let gb = file_size as f64 / 1_073_741_824.0;
    let rows_per_sec = rows as f64 / seconds;
    let gb_per_sec = gb / seconds;

    eprintln!();
    eprintln!("========== RUN STATS ==========");
    eprintln!("File: {}", path);
    eprintln!("File size: {:.2} GB", gb);
    eprintln!("Rows processed: {}", rows);
    eprintln!("Stations found: {}", station_count);
    eprintln!("\x1b[31mExecution time: {:.3?}\x1b[0m", elapsed);
    eprintln!("Rows/sec: {:.2} million", rows_per_sec / 1_000_000.0);
    eprintln!("Throughput: {:.2} GB/sec", gb_per_sec);
    eprintln!("===============================");
}
//fn next_line<'a>(map: &'a [u8], at: &mut usize) -> &'a [u8] {
//     let rest = &map[*at..];
//     // SAFETY: rest is valid for at least rest.len() bytes
//     let next_newline =
//         unsafe { libc::memchr(rest.as_ptr() as *const c_void, b'\n' as c_int, rest.len()) };
//     let line = if next_newline.is_null() {
//         // don't need to remember to break, since next iteration will find empty line
//         rest
//     } else {
//         // SAFETY: memchr always returns pointers in rest, which are valid
//         let len = unsafe { (next_newline as *const u8).offset_from(rest.as_ptr()) } as usize;
//         &rest[..len]
//     };
//     *at += line.len() + 1;
//     line
// }

fn next_line<'a>(map: &'a [u8], at: &mut usize) -> &'a [u8] {
    if *at >= map.len() {
        return &[];
    }

    let rest = &map[*at..];

    let next_newline =
        unsafe { libc::memchr(rest.as_ptr() as *const c_void, b'\n' as c_int, rest.len()) };

    if next_newline.is_null() {
        *at = map.len();
        rest
    } else {
        let len = unsafe { (next_newline as *const u8).offset_from(rest.as_ptr()) } as usize;
        *at += len + 1;
        &rest[..len]
    }
}
fn mmap(f: &File) -> Mmap {
    unsafe { Mmap::map(f).unwrap() }
}
fn split_semi(line: &[u8]) -> (&[u8], &[u8]) {
    // line.rsplit_once(|c| *c == b';').unwrap()
    // unsafe { line.rsplit_once(|&c| c == b';').unwrap_unchecked() }
    // we know, line is at most 100+1+5 = 106b
    let mut i = line.len() - 6;

    unsafe {
        while i < line.len() {
            if *line.get_unchecked(i) == b';' {
                return (line.get_unchecked(..i), line.get_unchecked(i + 1..));
            }
            i += 1;
        }

        std::hint::unreachable_unchecked()
    }
}
fn parse_temperature(temp: &[u8]) -> i16 {
    // let mut t: i32 = 0;
    // let mut mul: i32 = 1;

    // for &d in temperature.iter().rev() {
    //     match d {
    //         b'.' => {}
    //         b'-' => {
    //             t = -t;
    //             break;
    //         }
    //         b'0'..=b'9' => {
    //             t += i32::from(d - b'0') * mul;
    //             mul *= 10;
    //         }
    //         _ => panic!("bad temperature"),
    //     }
    // }
    // t
    match temp.len() {
        3 => {
            // 1.2
            ((temp[0] - b'0') as i16) * 10 + (temp[2] - b'0') as i16
        }
        4 => {
            if temp[0] == b'-' {
                // -1.2
                -(((temp[1] - b'0') as i16) * 10 + (temp[3] - b'0') as i16)
            } else {
                // 12.3
                ((temp[0] - b'0') as i16) * 100
                    + ((temp[1] - b'0') as i16) * 10
                    + (temp[3] - b'0') as i16
            }
        }
        5 => {
            // -12.3
            -(((temp[1] - b'0') as i16) * 100
                + ((temp[2] - b'0') as i16) * 10
                + (temp[4] - b'0') as i16)
        }
        _ => unsafe { std::hint::unreachable_unchecked() },
    }
}
    */
