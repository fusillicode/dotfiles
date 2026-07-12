use std::alloc::GlobalAlloc;
use std::alloc::Layout;
use std::alloc::System;
use std::hint::black_box;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use muxr_server::benchmark_support;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

struct CountingAllocator;

// SAFETY: every operation delegates to `System` with the original pointer and layout; atomics only observe requests.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(u64::try_from(layout.size()).unwrap_or(u64::MAX), Ordering::Relaxed);
        // SAFETY: this delegates the unchanged allocation request to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: these are the pointer and layout supplied by the matching allocation.
        unsafe { System.dealloc(ptr, layout) }
    }
}

fn benchmark_server_composer(c: &mut Criterion) {
    if let Err(error) = benchmark_support::verify_composer_oracle() {
        eprintln!("muxr server composer oracle failed: {error:?}");
        return;
    }
    let Ok(workloads) = benchmark_support::composer_workloads() else {
        eprintln!("failed to construct muxr server composer workloads");
        return;
    };
    for workload in workloads {
        for matrix in [
            benchmark_support::ComposerMatrix::ComposerOnly,
            benchmark_support::ComposerMatrix::EndToEnd,
        ] {
            let benchmark_name = format!("{}/{}", matrix.name(), workload.name());
            let Ok(mut sample_runner) = workload.runner(matrix) else {
                eprintln!("failed to prepare {benchmark_name}");
                continue;
            };
            // Allocation samples represent steady alternating updates, matching Criterion's persistent runner.
            let _warm_sample = sample_runner.step();
            benchmark_support::reset_composer_counters();
            ALLOCATIONS.store(0, Ordering::Relaxed);
            ALLOCATED_BYTES.store(0, Ordering::Relaxed);
            let Ok(sample) = sample_runner.step() else {
                eprintln!("failed to sample {benchmark_name}");
                continue;
            };
            eprintln!(
                "muxr-server-composer matrix={} workload={} allocations={} allocated_bytes={} pane_snapshots={} rows_initialized={} rows_recomposed={} cells_copied={} border_cells={}",
                matrix.name(),
                workload.name(),
                ALLOCATIONS.load(Ordering::Relaxed),
                ALLOCATED_BYTES.load(Ordering::Relaxed),
                sample.counters.pane_snapshots,
                sample.counters.rows_initialized,
                sample.counters.rows_recomposed,
                sample.counters.cells_copied,
                sample.counters.border_cells,
            );
            let Ok(mut runner) = workload.runner(matrix) else {
                continue;
            };
            c.bench_function(&benchmark_name, |b| b.iter(|| black_box(runner.step())));
        }
    }
}

criterion_group!(benches, benchmark_server_composer);
criterion_main!(benches);
