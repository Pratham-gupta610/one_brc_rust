use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{BufRead, BufReader, Write},
    time::Instant,
};
//
fn main() {
    let start = Instant::now();
    let path: &str = "data/measurements.txt";
    let file_size: u64 = std::fs::metadata(path).unwrap().len();

    let f: File = File::open(path).unwrap();
    let f: BufReader<File> = BufReader::new(f);

    let mut stats = HashMap::<Vec<u8>, (f64, f64, usize, f64)>::new();
    let mut rows = 0usize;

    for line in f.split(b'\n') {
        let line = line.unwrap();
        rows += 1;

        let mut fields = line.rsplitn(2, |c| *c == b';');

        let temperature = fields.next().unwrap();
        let station = fields.next().unwrap();

        // SAFETY: the README promised
        let temperature: f64 = unsafe { std::str::from_utf8_unchecked(temperature) }
            .parse()
            .unwrap();

        let stats = match stats.get_mut(station) {
            Some(stats) => stats,
            None => stats
                .entry(station.to_vec())
                .or_insert((f64::MAX, 0., 0, f64::MIN)),
        };

        stats.0 = stats.0.min(temperature);
        stats.1 += temperature;
        stats.2 += 1;
        stats.3 = stats.3.max(temperature);
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
        print!("{station}={min:.1}/{:.1}/{max:.1}", sum / (count as f64));

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
    eprintln!("Execution time: {:.3?}", elapsed);
    eprintln!("Rows/sec: {:.2} million", rows_per_sec / 1_000_000.0);
    eprintln!("Throughput: {:.2} GB/sec", gb_per_sec);
    eprintln!("===============================");
}
