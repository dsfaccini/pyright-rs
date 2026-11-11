/// mimalloc frees faster than glibc's allocator: the collector drops about 110M
/// evaluator objects on pydantic-ai, and their owned buffers dominate the sweep.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    std::process::exit(pyright::pyright::main());
}
