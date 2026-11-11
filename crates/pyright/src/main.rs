/// mimalloc frees faster than glibc's allocator: the collector drops about 110M
/// evaluator objects on pydantic-ai, and their owned buffers dominate the sweep.
#[cfg(not(feature = "count-malloc"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `CountingAlloc` counts the bytes held, for the collection log. Counting every
/// allocation costs about 0.6% of the instructions on pydantic-ai, so it is a feature.
#[cfg(feature = "count-malloc")]
#[global_allocator]
static GLOBAL: jsrt::heap::CountingAlloc<mimalloc::MiMalloc> = jsrt::heap::CountingAlloc(mimalloc::MiMalloc);

fn main() {
    std::process::exit(pyright::pyright::main());
}
