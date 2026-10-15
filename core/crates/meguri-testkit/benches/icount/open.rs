//! The sample instruction-count suite (U12, R1864): the warm open of the
//! 10k collection, counted by gungraun under valgrind. `tools/mg
//! bench-icount` runs it in Docker, makes the collection first (counting a
//! generation under valgrind would take minutes) and turns gungraun's JSON
//! into a perf result, whose counts U241's gate compares with its baselines
//! (+5% fails). Area units add their files beside this one.
//!
//! Only the benchmark function is counted: the setup (finding the shared
//! collection, the working copy, the warm-up open) is not, and neither is
//! dropping what the function returns.
//!
//! Names: the group is the area and the function the operation, so the
//! perf result's name is `<group>.<function>` (`collection.open`, as in the
//! criterion suite); a bench id `cards_<size>` names the collection.

use std::hint::black_box;

use gungraun::prelude::*;
use meguri_storage::OpenedCollection;
use meguri_testkit::bench::fixtures::{self, WorkingCopy};

/// A working copy of the 10k collection, opened once already.
fn warm_copy(size: &str) -> WorkingCopy {
    let collection = fixtures::profile_collection(size).expect("the shared collection");
    let copy = collection.working_copy().expect("a working copy");
    fixtures::open(&copy.path)
        .expect("it opens")
        .close()
        .expect("it closes");
    copy
}

#[library_benchmark]
#[bench::cards_10k(args = ("10k"), setup = warm_copy)]
fn open(copy: WorkingCopy) -> (OpenedCollection, WorkingCopy) {
    let opened = fixtures::open(black_box(&copy.path)).expect("it opens");
    (black_box(opened), copy)
}

library_benchmark_group!(name = collection, benchmarks = open);

main!(
    config = LibraryBenchmarkConfig::default().pass_through_envs(["MEGURI_BENCH_FIXTURES", "TMPDIR"]);
    library_benchmark_groups = collection
);
