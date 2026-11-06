//! Prints single-core kernel rates; used to compare build profiles.
use std::sync::atomic::AtomicBool;
use std::time::Duration;
fn main() {
    let stop = AtomicBool::new(false);
    let d = Duration::from_millis(1500);
    println!("hash {:.0} MB/s  matmul {:.2} GFLOPS", nocturn_bench::cpu_hash(1, d, &stop), nocturn_bench::cpu_matmul(1, d, &stop));
}
