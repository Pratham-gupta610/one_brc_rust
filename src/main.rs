use memmap2::Mmap;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Write,
    time::Instant,
};

//
fn main() {
    let start = Instant::now();
    let path: &str = "data/measurements.txt";
    let file_size: u64 = std::fs::metadata(path).unwrap().len();

    let f: File = File::open(path).unwrap();
    // let f: BufReader<File> = BufReader::new(f);
    let map: Mmap = mmap(&f);

    let mut stats = HashMap::<Vec<u8>, (i32, i64, usize, i32)>::new();

    let mut at = 0;

    let mut rows = 0usize;

    // for line in map.split(|c| *c == b'\n')
    loop {
        let rest = &map[at..];
        // SAFETY: rest is valid for at least rest.len() bytes
        let next_newline =
            unsafe { libc::memchr(rest.as_ptr() as *const c_void, b'\n' as c_int, rest.len()) };

        let line = if next_newline.is_null() {
            //NOTE:  don't need to remember to break, since next iteration will find empty line
            rest
        } else {
            // SAFETY: memchr always returns pointers in rest, which are valid
            let len = unsafe { (next_newline as *const u8).offset_from(rest.as_ptr()) } as usize;
            &rest[..len]
        };

        at += line.len() + 1;

        if line.is_empty() {
            break;
        }

        // if line.is_empty() {
        //     continue;
        // }
        rows += 1;

        let mut fields = line.rsplitn(2, |c| *c == b';');
        let (Some(temperature), Some(station)) = (fields.next(), fields.next()) else {
            panic!("bad line: {}", unsafe {
                std::str::from_utf8_unchecked(line)
            });
        };

        // SAFETY: the README promised

        let t = parse_temperature(temperature);
        let stats = match stats.get_mut(station) {
            Some(stats) => stats,
            None => stats
                .entry(station.to_vec())
                .or_insert((i32::MAX, 0, 0, i32::MIN)),
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
fn mmap(f: &File) -> Mmap {
    unsafe { Mmap::map(f).unwrap() }
}
fn parse_temperature(temperature: &[u8]) -> i32 {
    let mut t: i32 = 0;
    let mut mul: i32 = 1;

    for &d in temperature.iter().rev() {
        match d {
            b'.' => {}
            b'-' => {
                t = -t;
                break;
            }
            b'0'..=b'9' => {
                t += i32::from(d - b'0') * mul;
                mul *= 10;
            }
            _ => panic!("bad temperature"),
        }
    }
    t
}
