#![feature(portable_simd)]
#![feature(slice_split_once)]

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

struct FastHasherBuilder;
struct FastHasher(u64);

impl BuildHasher for FastHasherBuilder {
    type Hasher = FastHasher;

    fn build_hasher(&self) -> Self::Hasher {
        FastHasher(0xcbf29ce484222325)
    }
}

impl Hasher for FastHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, remainder) = bytes.as_chunks::<8>();
        let mut last = [1u8; 8];
        (last[..remainder.len()]).copy_from_slice(remainder);
        for &chunk in chunks.iter().chain(std::iter::once(&last)) {
            let mixed = self.0 as u128 * (u64::from_ne_bytes(chunk) as u128);
            self.0 = (mixed >> 64) as u64 ^ mixed as u64;
        }
    }
}
fn main() {
    let start = Instant::now();
    let path: &str = "data/measurements.txt";
    let file_size: u64 = std::fs::metadata(path).unwrap().len();

    let f: File = File::open(path).unwrap();
    // let f: BufReader<File> = BufReader::new(f);
    let map: Mmap = mmap(&f);
    // NOTE: maybe make the key &[u8], but measure since we're breaking MADV_SEQUENTIAL

    let mut stats = HashMap::<Vec<u8>, (i16, i64, usize, i16), _>::with_capacity_and_hasher(
        100_000,
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
    unsafe { line.rsplit_once(|&c| c == b';').unwrap_unchecked() }
    // we know, line is at most 100+1+5 = 106b
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
