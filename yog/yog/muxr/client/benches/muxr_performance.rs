use std::alloc::GlobalAlloc;
use std::alloc::Layout;
use std::alloc::System;
use std::hint::black_box;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use criterion::BatchSize;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use muxr_client::benchmark_support;
use muxr_transport::benchmark_support::PreparedSendFrames;
use muxr_transport::benchmark_support::SendPathBenchmark;
use muxr_transport::benchmark_support::SendPathMode;
use muxr_transport::benchmark_support::probe_send_path;

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
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
        self::fail_benchmark("failed to construct muxr transport benchmark runtime");
    };

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

        self::benchmark_transport(c, &workload, &runtime);

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

fn benchmark_transport(c: &mut Criterion, workload: &benchmark_support::Workload, runtime: &tokio::runtime::Runtime) {
    for mode in [SendPathMode::CopyingCodec, SendPathMode::Vectored] {
        let Ok(frames) = workload.transport_frames() else {
            self::fail_benchmark(&format!(
                "failed to encode transport proof workload {}",
                workload.name()
            ));
        };
        let Ok(proof) = runtime.block_on(probe_send_path(PreparedSendFrames::new(frames), mode)) else {
            self::fail_benchmark(&format!(
                "failed to verify transport send path: mode={} workload={}",
                mode.name(),
                workload.name()
            ));
        };
        eprintln!(
            "muxr-transport-send-proof mode={} workload={} frames={} wire_bytes={} payload_copies={} payload_bytes_copied={}",
            mode.name(),
            workload.name(),
            proof.frames,
            proof.wire_bytes,
            proof.payload_copies,
            proof.payload_bytes_copied,
        );
        self::benchmark_cold_transport(c, workload, mode, runtime);
        self::benchmark_reused_transport(c, workload, mode, runtime);
    }
}

fn benchmark_cold_transport(
    c: &mut Criterion,
    workload: &benchmark_support::Workload,
    mode: SendPathMode,
    runtime: &tokio::runtime::Runtime,
) {
    let Ok(frames) = workload.transport_frames() else {
        self::fail_benchmark(&format!("failed to encode transport workload {}", workload.name()));
    };
    let frames = PreparedSendFrames::new(frames);
    // Construct both writer states before measuring so this case isolates their first send without comparing
    // asymmetric connection/split/read-half setup.
    let mut benchmark = SendPathBenchmark::new(mode);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    let Ok(sample) = runtime.block_on(benchmark.run(frames)) else {
        self::fail_benchmark(&format!(
            "failed to sample cold transport send path: mode={} workload={}",
            mode.name(),
            workload.name()
        ));
    };
    eprintln!(
        "muxr-transport-send lifecycle=cold mode={} workload={} allocations={} allocated_bytes={} frames={} wire_bytes={}",
        mode.name(),
        workload.name(),
        ALLOCATIONS.load(Ordering::Relaxed),
        ALLOCATED_BYTES.load(Ordering::Relaxed),
        sample.frames,
        sample.wire_bytes,
    );
    let name = format!("transport_send_path/cold/{}/{}", mode.name(), workload.name());
    c.bench_function(&name, |b| {
        b.iter_batched_ref(
            || match workload.transport_frames() {
                Ok(frames) => (SendPathBenchmark::new(mode), Some(PreparedSendFrames::new(frames))),
                Err(error) => self::fail_benchmark(&format!(
                    "failed to prepare cold transport frames: workload={} error={error:?}",
                    workload.name()
                )),
            },
            |(benchmark, frames)| {
                let Some(frames) = frames.take() else {
                    self::fail_benchmark("cold transport benchmark reused one-shot prepared frames");
                };
                match runtime.block_on(benchmark.run(frames)) {
                    Ok(result) => black_box(result),
                    Err(error) => self::fail_benchmark(&format!(
                        "cold transport send path failed: mode={} workload={} error={error:?}",
                        mode.name(),
                        workload.name()
                    )),
                }
            },
            BatchSize::SmallInput,
        );
    });
}

fn benchmark_reused_transport(
    c: &mut Criterion,
    workload: &benchmark_support::Workload,
    mode: SendPathMode,
    runtime: &tokio::runtime::Runtime,
) {
    let Ok(frames) = workload.transport_frames() else {
        self::fail_benchmark(&format!("failed to encode transport workload {}", workload.name()));
    };
    let frames = PreparedSendFrames::new(frames);
    let mut benchmark = SendPathBenchmark::new(mode);
    if runtime.block_on(benchmark.run(frames)).is_err() {
        self::fail_benchmark(&format!(
            "failed to warm reused transport send path: mode={} workload={}",
            mode.name(),
            workload.name()
        ));
    }
    let Ok(sample_frames) = workload.transport_frames() else {
        self::fail_benchmark(&format!(
            "failed to prepare reused transport sample {}",
            workload.name()
        ));
    };
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    let Ok(sample) = runtime.block_on(benchmark.run(PreparedSendFrames::new(sample_frames))) else {
        self::fail_benchmark(&format!(
            "failed to sample reused transport send path: mode={} workload={}",
            mode.name(),
            workload.name()
        ));
    };
    eprintln!(
        "muxr-transport-send lifecycle=reused mode={} workload={} allocations={} allocated_bytes={} frames={} wire_bytes={}",
        mode.name(),
        workload.name(),
        ALLOCATIONS.load(Ordering::Relaxed),
        ALLOCATED_BYTES.load(Ordering::Relaxed),
        sample.frames,
        sample.wire_bytes,
    );
    let name = format!("transport_send_path/reused/{}/{}", mode.name(), workload.name());
    c.bench_function(&name, |b| {
        b.iter_batched(
            || match workload.transport_frames() {
                Ok(frames) => PreparedSendFrames::new(frames),
                Err(error) => self::fail_benchmark(&format!(
                    "failed to prepare reused transport frames: workload={} error={error:?}",
                    workload.name()
                )),
            },
            |frames| match runtime.block_on(benchmark.run(frames)) {
                Ok(result) => black_box(result),
                Err(error) => self::fail_benchmark(&format!(
                    "reused transport send path failed: mode={} workload={} error={error:?}",
                    mode.name(),
                    workload.name()
                )),
            },
            BatchSize::SmallInput,
        );
    });
}

fn fail_benchmark(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

criterion_group!(benches, benchmark_muxr);
criterion_main!(benches);
