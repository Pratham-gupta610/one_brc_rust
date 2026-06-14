One Billion Row Challenge in Rust

A high-performance Rust implementation of the One Billion Row Challenge (1BRC) focused on low-level systems optimization, custom data structures, cache-aware design, and parallel file processing.

This repository is not just a solution.

It is a deep performance-engineering exercise.

The goal was to understand:

- where time actually goes
- how memory layout affects performance
- why CPU caches matter
- why branch prediction matters
- why multithreading is not “free”
- why profiling is mandatory

---

What is 1BRC?

The official challenge:

https://github.com/gunnarmorling/1brc

The task:

Parse 1 billion weather measurements from a text file.

Each row looks like:

StationName;Temperature

Example:

Hamburg;12.4
Berlin;-3.1
Tokyo;27.8
Mumbai;36.1

The program must compute:

- minimum temperature
- maximum temperature
- average temperature

for every station.

Output:

{Berlin=-3.1/4.2/10.5, Hamburg=8.3/12.4/16.2}

---

Why is this hard?

At first glance:

This looks trivial.

Read lines.
Split strings.
Store stats.

Done.

Wrong.

The dataset contains:

1,000,000,000 rows

File size:

~13GB

At this scale:

Tiny inefficiencies become catastrophic.

Examples:

- one extra branch
- one extra allocation
- one unnecessary UTF-8 validation
- one slow hash lookup

Multiply that by:

1,000,000,000

That’s the real challenge.

---

Project Constraints

This implementation intentionally avoids external performance crates.

Allowed:

- Rust standard library
- "libc"

Not used:

- "memmap"
- "rayon"
- "ahash"
- "hashbrown"
- "memchr"
- "bytes"
- "crossbeam"

This means:

Everything performance-critical had to be written manually.

That includes:

- memory mapping
- parsing logic
- hash table logic
- hashing strategy
- chunk partitioning
- thread scheduling

This makes the challenge much harder.

But much more educational.

---

Performance Timeline

Phase 1 — Basic Implementation

Initial version:

- normal file reads
- standard parsing
- default HashMap
- string conversions

Best performance:

110s

Goal:

Just solve the problem correctly.

No optimization.

---

Phase 2 — Single-thread Optimization

Changes:

- manual mmap via "libc"
- raw byte slicing
- manual line scanning
- custom hash table
- custom hasher
- manual temperature parser

Best performance:

18.5s

Biggest learning:

Abstraction removal matters.

A lot.

---

Phase 3 — Multithreaded Processing

Changes:

- chunk-based file partitioning
- per-thread local aggregation
- zero shared writes
- single merge pass

Best performance:

3.1–3.4s

Final speedup:

110s → 3.2s (~35x faster)

---

Hardware Used

Machine:

CPU: Intel i7-14700HX
Cores: 20
Threads: 28
RAM: 16GB
OS: WSL2 Ubuntu
Rust: Nightly

Sweet spot:

20–22 worker threads

Important lesson:

More threads != more speed.

Past that point:

- scheduler overhead increased
- memory bandwidth became saturated
- performance dropped

---

Architecture

---

File Input

Uses manual "mmap" through "libc".

Why?

Traditional file reads:

- add syscall overhead
- add buffer copies

Memory mapping allows:

- zero-copy access
- OS-managed paging
- direct byte traversal

This significantly improved throughput.

---

Parsing Strategy

Parsing is done directly on raw bytes.

Avoided:

- "String"
- UTF-8 validation
- "split()"
- "parse::<f64>()"

Instead:

- locate ";"
- locate "\n"
- slice directly
- parse bytes manually

This removed massive overhead.

---

Custom Hash Table

Default HashMap was not ideal for this workload.

Reasons:

- generic overhead
- allocation behavior
- collision behavior

Implemented:

- fixed-size table
- linear probing
- cache-friendly layout

Benefits:

- predictable memory access
- fewer allocations
- better locality

---

Custom Hasher

A lightweight custom hasher built specifically for station names.

Why?

Default hashers are generalized.

This workload:

- has short strings
- repetitive lookups
- limited station cardinality

That allows specialization.

Result:

Lower hash overhead.

---

Temperature Parser

Standard parsing is expensive.

Instead:

Converted:

"-12.3"

directly into:

-123

Using byte arithmetic.

No floating-point parsing.

No dynamic checks.

Much faster.

---

Parallel Execution Model

The file is split into independent chunks.

Each worker:

1. gets a chunk
2. finds valid line boundaries
3. processes independently
4. builds local stats

No locking during hot-path execution.

Only one final merge.

This minimizes contention.

---

Profiling Workflow

Optimization was entirely data-driven.

Never guessed.

Always measured.

---

htop

Used for:

- CPU saturation
- thread balancing
- utilization analysis

Questions answered:

- Are all cores active?
- Are some workers idle?
- Is load balanced?

---

pidstat

Used for:

- context switches
- scheduling behavior
- thread efficiency

Questions answered:

- Is the scheduler hurting us?
- Are threads bouncing too much?

---

perf

Most important tool.

Used for:

- hotspot discovery
- instruction-level bottlenecks
- cycle attribution

Commands:

perf record ./target/release/one_brc_rust
perf report

This exposed:

- expensive parsing paths
- hash lookup overhead
- hidden allocations
- branch-heavy sections

This guided almost every major optimization.

---

Lessons Learned

1. More threads do not guarantee better performance

Scaling stops.

Then reverses.

Why?

Because hardware has limits.

Mostly:

- memory bandwidth
- cache pressure
- scheduler overhead

---

2. Better-looking code is not always faster

Many rewrites:

looked cleaner

but performed worse.

Performance engineering punishes assumptions.

---

3. Profilers tell the truth

Your intuition is often wrong.

The profiler isn’t.

---

Future Work

Current target:

Sub-1 second

To get there:

- SIMD newline scanning
- SIMD semicolon scanning
- branchless parser
- smaller table entries
- better cache locality
- smarter fingerprinting
- AVX2/AVX512 experiments
- NUMA-aware scaling
- huge pages
- native Linux testing
- bare-metal cloud benchmarking

Planned server:

Hetzner AX161
AMD EPYC
32C / 64T

Goal:

Measure real server-grade scaling.

---

Build

Requirements:

- Rust nightly
- Linux or WSL2

Build:

cargo +nightly build --release

Run:

./target/release/one_brc_rust

Benchmark:

time ./target/release/one_brc_rust

---

Why this repo exists

This repository documents my journey into:

- systems programming
- CPU-aware optimization
- memory-efficient design
- cache-conscious data structures
- Rust performance engineering

This project taught me more about real-world optimization than most tutorials ever could.

And it’s still far from finished.
