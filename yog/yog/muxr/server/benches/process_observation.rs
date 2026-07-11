use std::hint::black_box;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

fn benchmark_process_observation(c: &mut Criterion) {
    c.bench_function("process_observation_current_pid", |b| {
        b.iter(|| black_box(muxr_server::benchmark_support::observe_current_process()));
    });
}

criterion_group!(benches, benchmark_process_observation);
criterion_main!(benches);
