/// mimalloc frees faster than glibc's allocator: the collector drops about 110M
/// evaluator objects on pydantic-ai, and their owned buffers dominate the sweep.
/// Miri cannot run mimalloc's C code, so it keeps the system allocator.
#[cfg(not(any(feature = "count-malloc", miri)))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `CountingAlloc` counts the bytes held, for the collection log. Counting every
/// allocation costs about 0.6% of the instructions on pydantic-ai, so it is a feature.
#[cfg(all(feature = "count-malloc", not(miri)))]
#[global_allocator]
static GLOBAL: jsrt::heap::CountingAlloc<mimalloc::MiMalloc> = jsrt::heap::CountingAlloc(mimalloc::MiMalloc);
#[cfg(all(feature = "count-malloc", miri))]
#[global_allocator]
static GLOBAL: jsrt::heap::CountingAlloc<std::alloc::System> = jsrt::heap::CountingAlloc(std::alloc::System);

fn main() {
    std::process::exit(pyright::pyright::main());
}
