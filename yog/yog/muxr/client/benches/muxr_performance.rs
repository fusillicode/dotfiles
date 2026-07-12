use std::alloc::GlobalAlloc;
use std::alloc::Layout;
use std::alloc::System;
use std::hint::black_box;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use muxr_client::benchmark_support;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

struct CountingAllocator;

// SAFETY: every operation delegates to `System` with the original pointer and layout; the atomics are observation only.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(u64::try_from(layout.size()).unwrap_or(u64::MAX), Ordering::Relaxed);
        // SAFETY: this allocator delegates the unchanged allocation request to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` are the values supplied by the matching system allocation.
        unsafe { System.dealloc(ptr, layout) }
    }
}

fn benchmark_muxr(c: &mut Criterion) {
    // Test-only fault injection verifies that Criterion cannot report success after a preflight failure.
    if std::env::var_os("MUXR_BENCH_TEST_FAILURE").is_some() {
        self::fail_benchmark("injected muxr benchmark preflight failure");
    }
    let Ok(workloads) = benchmark_support::workloads() else {
        self::fail_benchmark("failed to construct muxr benchmark workloads");
    };
    if let Err(error) = benchmark_support::verify_oracle() {
        self::fail_benchmark(&format!("muxr benchmark oracle failed: {error:?}"));
    }

    for workload in workloads {
        ALLOCATIONS.store(0, Ordering::Relaxed);
        ALLOCATED_BYTES.store(0, Ordering::Relaxed);
        let Ok(sample) = workload.run() else {
            self::fail_benchmark(&format!("failed to sample muxr workload {}", workload.name()));
        };
        eprintln!(
            "muxr-baseline workload={} allocations={} allocated_bytes={} encoded_bytes={} frames_encoded={} render_cells_encoded={} dirty_cells_encoded={} terminal_bytes={}",
            workload.name(),
            ALLOCATIONS.load(Ordering::Relaxed),
            ALLOCATED_BYTES.load(Ordering::Relaxed),
            sample.encoded_bytes,
            sample.counters.frames_encoded,
            sample.counters.render_cells_encoded,
            sample.counters.dirty_cells_encoded,
            sample.terminal_bytes.len(),
        );
        c.bench_function(workload.name(), |b| {
            b.iter(|| match workload.run() {
                Ok(result) => black_box(result),
                Err(error) => self::fail_benchmark(&format!(
                    "muxr benchmark workload failed: workload={} error={error:?}",
                    workload.name()
                )),
            });
        });

        if workload.has_render_updates() {
            for mode in [
                benchmark_support::ClientTransactionMode::Fresh,
                benchmark_support::ClientTransactionMode::Reused,
            ] {
                ALLOCATIONS.store(0, Ordering::Relaxed);
                ALLOCATED_BYTES.store(0, Ordering::Relaxed);
                let Ok(sample) = workload.run_client_transactions(mode) else {
                    self::fail_benchmark(&format!(
                        "failed to sample muxr client transactions: mode={} workload={}",
                        mode.name(),
                        workload.name()
                    ));
                };
                eprintln!(
                    "muxr-client-transactions mode={} workload={} allocations={} allocated_bytes={} frames_rendered={} terminal_bytes={} retained_transaction_bytes={}",
                    mode.name(),
                    workload.name(),
                    ALLOCATIONS.load(Ordering::Relaxed),
                    ALLOCATED_BYTES.load(Ordering::Relaxed),
                    sample.frames_rendered,
                    sample.terminal_bytes.len(),
                    sample.retained_transaction_bytes,
                );
                let benchmark_name = format!("client_transactions/{}/{}", mode.name(), workload.name());
                c.bench_function(&benchmark_name, |b| {
                    b.iter(|| match workload.run_client_transactions(mode) {
                        Ok(result) => black_box(result),
                        Err(error) => self::fail_benchmark(&format!(
                            "muxr client transaction benchmark failed: mode={} workload={} error={error:?}",
                            mode.name(),
                            workload.name()
                        )),
                    });
                });
            }
        }
    }
}

fn fail_benchmark(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

criterion_group!(benches, benchmark_muxr);
criterion_main!(benches);
