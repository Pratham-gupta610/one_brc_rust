
# 1BRC in Rust — 13 GB, 1 Billion Rows, 3.2 Seconds

> **One Billion Row Challenge** — parse a 13 GB file of weather station readings, compute min/max/mean per station. No external crates. No magic libraries. Just Rust and raw systems primitives.

---

## Results

| Phase | Approach | Best Time |
|---|---|---|
| Phase 1 | Naive — `BufReader` + `HashMap` | ~110s |
| Phase 2 | Single-threaded, fully optimized | ~18.5s |
| Phase 3 | Multithreaded (18–22 threads) | ~3.1–3.3s |

**~35× total speedup over the baseline.**

Hardware: Intel i7-14700HX · 20 cores / 28 threads · 16 GB RAM · WSL2 Ubuntu · Rust nightly

---

## The Challenge

The input file has this structure:

```
Hamburg;12.0
Bulawayo;8.9
Palembang;38.8
... (×1,000,000,000)
```

The output must be sorted alphabetically:

```
{Hamburg=12.0/12.0/12.0, Palembang=38.8/38.8/38.8, ...}
```

Rules that made this hard:
- **No external crates.** No `rayon`, no `memmap2`, no `ahash`, no `nom`.
- Everything you use must be built or called directly through `libc` or `std`.

---

## How I Removed 106 Seconds

Not by guessing. By measuring.

Tools used at each phase:
- `htop` — live core utilization to spot thread imbalance
- `pidstat -u 1` — per-thread CPU scheduling and context switch rate
- `perf stat` / `perf record` + `perf report` — CPU hotspot analysis, cache miss rates, branch mispredictions

The perf report from the best single-threaded run was what drove most Phase 2 decisions. Without it I would have been optimizing the wrong things.

---

## Architecture

### Phase 1 — Baseline (~110s)

Standard idiomatic Rust. `BufReader` over the file, `split(';')`, `parse::<f64>()`, insertions into a `HashMap<String, (f64, f64, f64, u32)>`.

This was about getting a correct answer first. Nothing more.

**Bottlenecks identified:**
- `BufReader` copies data into a userspace buffer on every read syscall
- `parse::<f64>()` is a full general-purpose float parser — overkill for `"-12.3"` format
- `String` allocation per station name on every row
- `HashMap`'s default `SipHash` hasher is designed for DoS resistance, not throughput

---

### Phase 2 — Single-threaded Optimized (~18.5s)

Five concrete changes drove the entire 6× improvement.

#### 1. `mmap` instead of `BufReader`

```rust
let ptr = libc::mmap(
    std::ptr::null_mut(),
    len,
    libc::PROT_READ,
    libc::MAP_SHARED,
    f.as_raw_fd(),
    0,
);
libc::madvise(ptr, len, libc::MADV_SEQUENTIAL);
libc::madvise(ptr, len, libc::MADV_HUGEPAGE);
```

`mmap` maps the file directly into the process's virtual address space. The OS page cache handles I/O; there's no `read()` syscall overhead and no userspace copy. The kernel reads ahead aggressively with `MADV_SEQUENTIAL`. `MADV_HUGEPAGE` enables 2 MB TLB pages, reducing TLB pressure on a 13 GB working set.

The entire file becomes a `&[u8]` — a slice you can index directly.

#### 2. Custom open-addressing hash table

```rust
const SLOTS: usize = 1 << 12; // 4096 slots
const MASK: usize = SLOTS - 1;
```

`std::HashMap` carries overhead per operation: hash computation, bucket search, `Box` allocation for entries. For this workload — ~400 unique station names, billions of lookups — we need a flat, cache-resident table with zero allocation after initialization.

The table is a fixed-size open-addressing array of `Entry` structs with linear probing. Station names are stored inline as `[u8; 100]` — no heap allocation, no pointer indirection.

**One important note:** 4096 slots for ~400 keys gives a load factor of ~10%. This is very low. Open-addressing degrades as load factor increases (more collisions, longer probe chains). At 10%, probe chains are almost always length 1. This is a deliberate trade-off: waste memory to get near-O(1) lookups.

#### 3. FNV-1a hash, computed inline during name scan

```rust
let mut h = 2166136261u32; // FNV offset basis

loop {
    let b = *data.get_unchecked(pos);
    if b == b';' { break; }
    h ^= b as u32;
    h = h.wrapping_mul(16777619); // FNV prime
    pos += 1;
}
```

FNV-1a (Fowler–Noll–Vo) is not cryptographic. It's fast, produces low collision rates on ASCII strings, and can be computed during the linear scan for the `';'` separator — no second pass over the name bytes needed.

`SipHash` (the default) does two passes and uses 128-bit state. For our use case that's unnecessary overhead.

#### 4. `libc::memchr` for byte search

```rust
fn find_byte(data: &[u8], byte: u8) -> usize {
    let p = unsafe { libc::memchr(data.as_ptr() as *const c_void, byte as c_int, data.len()) };
    ...
}
```

`memchr` from glibc uses SIMD (SSE2/AVX2) internally to scan 16–32 bytes per cycle. A naive Rust loop over bytes — even with LLVM auto-vectorization — won't reliably match it. Using `memchr` for newline detection hands that work to a battle-tested SIMD implementation.

#### 5. Branchless integer temperature parsing

The input format is constrained: one decimal digit after the dot, values in range `[-99.9, 99.9]`. We never need a general float parser.

```rust
// Temperatures are stored ×10 as i32 — no floats at all until output
if b0 == b'-' {
    let b2 = *data.get_unchecked(pos + 2);
    if b2 == b'.' {
        // Format: -X.Y  (e.g. -3.7)
        t = -(((*data.get_unchecked(pos + 1) - b'0') as i32) * 10
            + (*data.get_unchecked(pos + 3) - b'0') as i32);
        pos += 4;
    } else {
        // Format: -XX.Y (e.g. -38.2)
        t = -(((*data.get_unchecked(pos + 1) - b'0') as i32) * 100
            + ((*data.get_unchecked(pos + 2) - b'0') as i32) * 10
            + (*data.get_unchecked(pos + 4) - b'0') as i32);
        pos += 5;
    }
}
```

All four possible formats (`X.Y`, `XX.Y`, `-X.Y`, `-XX.Y`) are handled with fixed-offset byte reads. No loops, no allocation, no floating-point operations until the final output step where we divide by 10.

Storing as `i32` ×10 means min/max/sum operations are all integer arithmetic — cheaper than FP and exactly representable.

---

### Phase 3 — Multithreaded (~3.1–3.3s)

With single-core work already optimized, the remaining bottleneck was throughput: one core saturates at ~18.5s because it can only consume so much I/O and process so many bytes per second.

#### Chunk splitting with newline alignment

```rust
let chunk = data.len() / nthreads;

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
```

The file is divided into `nthreads` roughly equal byte slices. Each boundary is snapped forward to the next `'\n'` so no row is ever split across two threads. Each thread gets a `&[u8]` slice — a read-only reference into the already-mapped file.

#### Zero lock contention — no shared mutable state during processing

Each thread runs `process()` independently and builds its own local `Table`. There are no mutexes, no atomics, no channels during the processing phase. Threads never communicate.

This is the core architectural insight: **synchronization is the enemy**. Instead of sharing a central map with locks, we pay for N copies of the map (N × 4096 entries × ~130 bytes = ~2 MB total, cheap) and merge at the end.

```rust
// After all threads finish, merge sequentially
for handle in handles {
    let table = handle.join().unwrap();
    for (e, &h) in table.entries.iter().zip(table.hashes.iter()) {
        if h != 0 {
            merged.merge_entry(e, h);
        }
    }
}
```

The merge is O(N × 4096) — a few million cheap operations — not on the critical path.

#### Thread count sweet spot

Optimal thread count was empirically ~19–22 on this hardware, not 28 (the logical core count).

Why? Past a threshold, adding threads doesn't help because:
1. **Memory bandwidth saturation** — the file is 13 GB; DDR5 bandwidth (~50 GB/s peak, less in practice under WSL2) becomes the bottleneck, not compute
2. **Scheduler overhead** — more threads means more context switches, more cache evictions, more time spent in kernel scheduling
3. **Hyper-threading diminishing returns** — logical cores 21–28 are HT siblings sharing physical execution units; for memory-bandwidth-bound work they compete more than they cooperate

`available_parallelism()` returns logical cores. For I/O-bound work, using physical core count (or slightly above) is often better than using all logical cores.

---

## Data Structures

### `Entry`

```rust
struct Entry {
    key: [u8; 100],   // station name, inline — no heap allocation
    klen: u8,          // name length
    min: i32,          // temperature × 10
    max: i32,
    sum: i64,          // summed temperature × 10
    count: u32,
}
```

All fields are fixed-size. The entire struct is ~130 bytes, fits within a few cache lines. Keeping the name inline avoids pointer chasing — `memcmp` during lookup reads from a single contiguous region.

### `Table`

Two parallel arrays: `entries: Vec<Entry>` and `hashes: Vec<u32>`. A zero hash value means an empty slot (FNV hash results of 0 are remapped to 1 to preserve this invariant).

Lookup: `slot = hash & MASK`, linear probe on collision. With ~10% load factor, collisions are rare.

---

## What's Left — Path to Sub-1s

The current bottleneck is memory bandwidth. Every byte of the 13 GB file is read once. On this hardware, pushing below 3s significantly requires either reducing the bytes read or processing more bytes per cycle.

| Technique | Expected gain | Complexity |
|---|---|---|
| SIMD name scanning (AVX2) | Moderate | High |
| SIMD `'\n'` / `';'` detection | Moderate | High |
| Branchless hash via SIMD (compute hash over 16B at once) | Small–moderate | High |
| Cache-line-aligned `Entry` structs | Small | Low |
| Reduce `Entry` size (pack min/max/count into fewer bytes) | Small | Low |
| NUMA-aware memory allocation (server hardware) | Large on EPYC | Medium |
| io_uring for async prefetch | Uncertain under WSL2 | Very high |

On an AMD EPYC AX161 (32 cores / 64 threads, higher memory bandwidth), the threading headroom is much larger. That's the next benchmark target.

---

## Running

```bash
# Build (nightly required for some intrinsics)
cargo +nightly build --release

# Run with default thread count (available_parallelism)
./target/release/1brc data/measurements.txt

# Run with explicit thread count
THREADS=20 ./target/release/1brc data/measurements.txt
```

Output goes to stdout. Performance stats go to stderr:

```
═══════════════════════════════════════════════
               🚀 1BRC RUN STATS 🚀
═══════════════════════════════════════════════
📂 File Size   : 13.00 GB
🧵 Threads     : 20
⚡ Time Taken  : 3.213s
🔥 Throughput  : 4.05 GB/s
═══════════════════════════════════════════════
```

**Note:** `mmap` + `libc` calls are Linux/Unix only. This will not compile on Windows. Open from inside WSL2 (`code .` in the WSL terminal), or set the rust-analyzer target to `x86_64-unknown-linux-gnu`.

---

## Testing

```bash
cargo test
```

The `temp_manual_cases` test covers all four parsing branches: single-digit positive (`0.0`), single-digit negative (`-9.2`), two-digit positive (`98.2`), two-digit negative (`-98.2`), and verifies exact integer storage (values × 10).

---

## Lessons

**Abstractions have a cost.** `BufReader`, `String`, `HashMap`, `parse::<f64>()` — each one is convenient and each one has overhead. In hot paths, that overhead compounds across a billion iterations.

**Profile before you optimize.** Most early intuitions about where time is spent are wrong. `perf report` showed the actual hotspots; several things I expected to be fast were slow, and vice versa.

**More threads ≠ more performance.** At some point you run out of memory bandwidth, not compute. Adding threads past the saturation point just adds scheduler overhead.

**Correctness first.** Phase 1 existed so there was a known-correct baseline to diff against at every optimization step. Optimizing unverified code is how you get fast and wrong.

---

## References

- [Official 1BRC Repository](https://github.com/gunnarmorling/1brc)
- FNV-1a hash specification
- Linux `mmap(2)` and `madvise(2)` man pages
- *Systems Performance* — Brendan Gregg (for `perf` methodology)
