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
    let Ok(workloads) = benchmark_support::workloads() else {
        eprintln!("failed to construct muxr benchmark workloads");
        return;
    };
    if let Err(error) = benchmark_support::verify_oracle() {
        eprintln!("muxr benchmark oracle failed: {error:?}");
        return;
    }

    for workload in workloads {
        ALLOCATIONS.store(0, Ordering::Relaxed);
        ALLOCATED_BYTES.store(0, Ordering::Relaxed);
        let Ok(sample) = workload.run() else {
            eprintln!("failed to sample muxr workload {}", workload.name());
            continue;
        };
        eprintln!(
            "muxr-baseline workload={} allocations={} allocated_bytes={} encoded_bytes={} payload_copies={} panes_snapshotted={} cells_snapshotted={} terminal_bytes={}",
            workload.name(),
            ALLOCATIONS.load(Ordering::Relaxed),
            ALLOCATED_BYTES.load(Ordering::Relaxed),
            sample.encoded_bytes,
            sample.counters.payload_copies,
            sample.counters.panes_snapshotted,
            sample.counters.cells_snapshotted,
            sample.terminal_bytes.len(),
        );
        c.bench_function(workload.name(), |b| {
            b.iter(|| black_box(workload.run()));
        });
    }
}

criterion_group!(benches, benchmark_muxr);
criterion_main!(benches);
