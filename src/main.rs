use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Write,
    time::Instant,
};

use memmap2::Mmap;

//
fn main() {
    let start = Instant::now();
    let path: &str = "data/measurements.txt";
    let file_size: u64 = std::fs::metadata(path).unwrap().len();

    let f: File = File::open(path).unwrap();
    // let f: BufReader<File> = BufReader::new(f);
    let map: Mmap = mmap(&f);

    let mut stats = HashMap::<Vec<u8>, (i32, i64, usize, i32)>::new();
    let mut rows = 0usize;

    for line in map.split(|c| *c == b'\n') {
        //  let line = line.unwrap();
        if line.is_empty() {
            continue;
        }
        rows += 1;

        let mut fields = line.rsplitn(2, |c| *c == b';');
        let (Some(temperature), Some(station)) = (fields.next(), fields.next()) else {
            panic!("bad line: {}", unsafe {
                std::str::from_utf8_unchecked(line)
            });
        };

        // SAFETY: the README promised
        // let mut t: i32 = 0;
        // let mut mul: i32 = 1;

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
        // let temperature = fields.next().unwrap();
        // let station = fields.next().unwrap();

        // let temperature: f64 = unsafe { std::str::from_utf8_unchecked(temperature) }
        //     .parse()
        //     .unwrap();

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
    eprintln!("----Execution time----: {:.3?}", elapsed);
    eprintln!("Rows/sec: {:.2} million", rows_per_sec / 1_000_000.0);
    eprintln!("Throughput: {:.2} GB/sec", gb_per_sec);
    eprintln!("===============================");
}
fn mmap(f: &File) -> Mmap {
    unsafe { Mmap::map(f).unwrap() }
}
