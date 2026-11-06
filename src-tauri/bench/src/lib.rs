//! Benchmark kernels for NOCTURN. Pure computation only — no I/O, no OS
//! calls — so they can be built with full optimisation in every profile.
//!
//! Every kernel runs for a fixed wall-clock budget and returns a rate, so a
//! slow PC finishes as quickly as a fast one. `stop` lets the caller cancel.

use std::hint::black_box;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// Run `work` repeatedly on `threads` threads for `budget`; each call returns
/// the units of work it did. Returns total units per second.
fn timed<F>(threads: usize, budget: Duration, stop: &AtomicBool, work: F) -> f64
where
    F: Fn(usize) -> f64 + Sync,
{
    let start = Instant::now();
    let total: f64 = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads.max(1))
            .map(|t| {
                let work = &work;
                s.spawn(move || {
                    let mut units = 0.0;
                    while start.elapsed() < budget && !stop.load(Ordering::Relaxed) {
                        units += work(t);
                    }
                    units
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or(0.0)).sum()
    });
    total / start.elapsed().as_secs_f64().max(1e-9)
}

/// SHA-256 throughput in MB/s (integer and bit operations).
pub fn cpu_hash(threads: usize, budget: Duration, stop: &AtomicBool) -> f64 {
    let block = vec![0x5Au8; 1 << 20];
    timed(threads, budget, stop, |_| {
        let mut h = Sha256::new();
        h.update(&block);
        black_box(h.finalize());
        1.0
    })
}

/// Dense f64 matrix multiply throughput in GFLOPS (floating point).
pub fn cpu_matmul(threads: usize, budget: Duration, stop: &AtomicBool) -> f64 {
    const N: usize = 128;
    let a: Vec<f64> = (0..N * N).map(|i| (i % 7) as f64 * 0.5 + 1.0).collect();
    let b: Vec<f64> = (0..N * N).map(|i| (i % 5) as f64 * 0.25 + 1.0).collect();
    let flops = 2.0 * (N * N * N) as f64;
    timed(threads, budget, stop, |_| {
        let mut c = vec![0.0f64; N * N];
        for i in 0..N {
            for k in 0..N {
                let aik = a[i * N + k];
                let row = &b[k * N..k * N + N];
                let out = &mut c[i * N..i * N + N];
                for (o, bv) in out.iter_mut().zip(row) {
                    *o += aik * bv;
                }
            }
        }
        black_box(&c);
        flops / 1e9
    })
}

/// Memory bandwidth in GB/s: (read, write, copy), using every thread.
pub fn mem_bandwidth(bytes: usize, threads: usize, budget: Duration, stop: &AtomicBool) -> (f64, f64, f64) {
    let threads = threads.max(1);
    let words = (bytes / 8 / threads).max(1 << 16);
    let mut bufs: Vec<Vec<u64>> = (0..threads).map(|t| vec![t as u64; words]).collect();
    let mut dst: Vec<Vec<u64>> = (0..threads).map(|_| vec![0u64; words]).collect();
    let gb = (words * 8) as f64 / 1e9;

    // Read: sum every word.
    let read = {
        let bufs = &bufs;
        timed(threads, budget, stop, |t| {
            let s: u64 = bufs[t].iter().fold(0u64, |a, &x| a.wrapping_add(x));
            black_box(s);
            gb
        })
    };
    // Write and copy need exclusive buffers per thread.
    let start = Instant::now();
    let mut write_units = 0.0;
    let mut copy_units = 0.0;
    std::thread::scope(|s| {
        let hs: Vec<_> = bufs
            .iter_mut()
            .zip(dst.iter_mut())
            .map(|(src, d)| {
                s.spawn(move || {
                    let half = budget / 2;
                    let t0 = Instant::now();
                    let mut w = 0.0;
                    let mut v = 1u64;
                    while t0.elapsed() < half && !stop.load(Ordering::Relaxed) {
                        src.fill(v);
                        v = v.wrapping_add(1);
                        black_box(&src);
                        w += gb;
                    }
                    let wt = t0.elapsed().as_secs_f64();
                    let t1 = Instant::now();
                    let mut c = 0.0;
                    while t1.elapsed() < half && !stop.load(Ordering::Relaxed) {
                        d.copy_from_slice(src);
                        black_box(&d);
                        // A copy reads and writes every byte.
                        c += 2.0 * gb;
                    }
                    (w / wt.max(1e-9), c / t1.elapsed().as_secs_f64().max(1e-9))
                })
            })
            .collect();
        for h in hs {
            let (w, c) = h.join().unwrap_or((0.0, 0.0));
            write_units += w;
            copy_units += c;
        }
    });
    let _ = start;
    (read, write_units, copy_units)
}

/// Random-access memory latency in nanoseconds (dependent pointer chase over
/// a buffer far larger than the CPU caches).
pub fn mem_latency(bytes: usize, budget: Duration, stop: &AtomicBool) -> f64 {
    let n = (bytes / std::mem::size_of::<usize>()).max(1 << 16);
    // Sattolo's algorithm: one random cycle through every slot.
    let mut next: Vec<usize> = (0..n).collect();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in (1..n).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = (x % i as u64) as usize;
        next.swap(i, j);
    }
    let mut p = 0usize;
    let mut steps = 0u64;
    let t0 = Instant::now();
    while t0.elapsed() < budget && !stop.load(Ordering::Relaxed) {
        for _ in 0..100_000 {
            p = next[p];
        }
        steps += 100_000;
    }
    black_box(p);
    t0.elapsed().as_nanos() as f64 / steps.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernels_return_plausible_rates() {
        let stop = AtomicBool::new(false);
        let d = Duration::from_millis(150);
        assert!(cpu_hash(1, d, &stop) > 1.0);
        assert!(cpu_matmul(1, d, &stop) > 0.01);
        let (r, w, c) = mem_bandwidth(8 << 20, 2, d, &stop);
        assert!(r > 0.0 && w > 0.0 && c > 0.0);
        let lat = mem_latency(8 << 20, d, &stop);
        assert!(lat > 0.1 && lat < 10_000.0, "{lat}");
    }

    #[test]
    fn stop_ends_early() {
        let stop = AtomicBool::new(true);
        let t = Instant::now();
        cpu_hash(2, Duration::from_secs(10), &stop);
        assert!(t.elapsed() < Duration::from_secs(1));
    }
}
