//! `PYRIGHT_RS_GC_CENSUS=1`: a breakdown of the resident set, for finding where
//! memory goes (`docs/collector-design.md`, "census"). It changes no result: the
//! snapshots only read, and the step that drops objects runs at exit, after the
//! output is written.
//!
//! Buffer bytes come from the malloc counter, so they need the `pyright` crate's
//! `count-malloc` feature; without it the census says so and skips them.

use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::vm::resident;

use super::{
    BLOCK, BLOCK_SHIFT, BlockMeta, CARD_SHIFT, Collection, GcMode, START, ThreadHeap, config, lock, malloc_bytes, ranges,
    rss_bytes, with_heap,
};

pub(super) const MUTATOR: u8 = 0;
pub(super) const MARKING: u8 = 1;
pub(super) const SWEEPING: u8 = 2;

static PHASE: AtomicU8 = AtomicU8::new(MUTATOR);
/// The largest resident set the sampler saw in each phase, and when (ms since start).
static PEAKS: Mutex<[(usize, u64); 3]> = Mutex::new([(0, 0); 3]);

/// Whether the census is on. The first call starts a thread that samples the
/// resident set every 5 ms.
pub(super) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var_os("PYRIGHT_RS_GC_CENSUS").is_some_and(|v| !v.is_empty() && v != "0");
        if on {
            START.get_or_init(Instant::now);
            std::thread::spawn(|| {
                loop {
                    let rss = rss_bytes();
                    let phase = PHASE.load(Relaxed) as usize;
                    let mut peaks = lock(&PEAKS);
                    if rss > peaks[phase].0 {
                        peaks[phase] = (rss, START.get_or_init(Instant::now).elapsed().as_millis() as u64);
                    }
                    drop(peaks);
                    std::thread::sleep(Duration::from_millis(5));
                }
            });
        }
        on
    })
}

pub(super) fn set_phase(phase: u8) {
    PHASE.store(phase, Relaxed);
}

/// Resident bytes of file-backed mappings (the binary, libraries) and of the main stack.
fn file_and_stack_rss() -> (usize, usize) {
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
    let (mut kind, mut file, mut stack) = (0u8, 0usize, 0usize);
    for line in smaps.lines() {
        let mut fields = line.split_whitespace();
        let first = fields.next().unwrap_or("");
        if first.split_once('-').is_some_and(|(start, _)| usize::from_str_radix(start, 16).is_ok()) {
            let path = fields.nth(4).unwrap_or("");
            kind = if path.starts_with('/') { 1 } else if path.starts_with("[stack") { 2 } else { 0 };
        } else if first == "Rss:" {
            let bytes = fields.next().and_then(|kib| kib.parse::<usize>().ok()).unwrap_or(0) << 10;
            match kind {
                1 => file += bytes,
                2 => stack += bytes,
                _ => {}
            }
        }
    }
    (file, stack)
}

fn mib(bytes: usize) -> usize {
    bytes >> 20
}

fn seconds() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// The malloc counter, or `None` when the binary does not count (a thread that
/// allocated always holds some bytes).
fn counted() -> Option<usize> {
    Some(malloc_bytes()).filter(|&bytes| bytes > 0)
}

impl ThreadHeap {
    /// Prints the resident set by part. With `marks`, the mark bits tell reachable
    /// slots from garbage: right after a mark, before its sweep.
    pub(super) fn census_snapshot(&self, label: &str, marks: bool) {
        let ranges = ranges();
        let rss = rss_bytes();
        let (mut blocks, mut allocated, mut marked, mut bitmaps) = (0usize, 0usize, 0usize, 0usize);
        for class in self.eval.iter().flatten() {
            let slot = class.info.slot as usize;
            for &block in &class.blocks {
                // SAFETY: a block of this class, owned by this thread.
                let meta = unsafe { ranges.eval.meta(block) };
                let words = meta.words as usize;
                blocks += BLOCK;
                bitmaps += meta.cap as usize * 8;
                for w in 0..words {
                    // SAFETY: the live and mark bitmaps of the block, `words` words each.
                    let (live, mark) = unsafe { (*meta.bits.add(w), *meta.bits.add(words + w)) };
                    allocated += live.count_ones() as usize * slot;
                    marked += (live & mark).count_ones() as usize * slot;
                }
            }
        }
        let mut program_used = 0;
        for class in self.program.iter().flatten() {
            for &block in &class.blocks {
                // SAFETY: as above.
                program_used += unsafe { ranges.program.meta(block) }.bump as usize * class.info.slot as usize;
            }
        }
        let (warm, eval_committed) = {
            let pool = lock(&ranges.eval.blocks);
            (pool.warm.clone(), pool.committed << BLOCK_SHIFT)
        };
        let program_committed = lock(&ranges.program.blocks).committed << BLOCK_SHIFT;
        let eval_rss = resident(ranges.eval.base, eval_committed);
        let warm_rss: usize = warm.iter().map(|&block| resident(ranges.eval.block_base(block), BLOCK)).sum();
        let program_rss = resident(ranges.program.base, program_committed);
        let meta_rss = resident(ranges.eval.meta as usize, (eval_committed >> BLOCK_SHIFT) * size_of::<BlockMeta>())
            + resident(ranges.eval.cards, eval_committed >> CARD_SHIFT)
            + resident(ranges.program.meta as usize, (program_committed >> BLOCK_SHIFT) * size_of::<BlockMeta>())
            + resident(ranges.program.cards, program_committed >> CARD_SHIFT);
        let (arena_used, arena_rss) = crate::arena::arena_chunks_resident();
        let chunks = crate::arena::arena_chunk_bytes();
        let (file, stack) = file_and_stack_rss();
        let rest = rss.saturating_sub(eval_rss + program_rss + meta_rss + arena_rss + file + stack);
        let reachable = if marks {
            format!("reachable {} MiB, garbage {} MiB, ", mib(marked), mib(allocated - marked))
        } else {
            String::new()
        };
        let malloc = match counted() {
            Some(bytes) => format!(
                "{} MiB requested: arena chunks {} MiB, the rest {} MiB (evaluator bitmaps {} MiB)",
                mib(bytes),
                mib(chunks),
                mib(bytes.saturating_sub(chunks)),
                mib(bitmaps)
            ),
            None => "not counted (count-malloc)".to_string(),
        };
        eprintln!(
            "census {label} at {:.1} s: rss {} MiB = evaluator range {} MiB [blocks in use {} MiB: slots {} MiB ({reachable}free {} MiB); \
             warm released blocks {} MiB] + program range {} MiB ({} MiB of objects) + block metadata and cards {} MiB \
             + program arena {} MiB resident ({} MiB used, {} MiB of chunks) + file-backed {} MiB + stack {} MiB \
             + malloc heap and the rest {} MiB; malloc {malloc}",
            seconds(),
            mib(rss),
            mib(eval_rss),
            mib(blocks),
            mib(allocated),
            mib(blocks - allocated),
            mib(warm_rss),
            mib(program_rss),
            mib(program_used),
            mib(meta_rss),
            mib(arena_rss),
            mib(arena_used),
            mib(chunks),
            mib(file),
            mib(stack),
            mib(rest),
        );
    }

    /// At exit: a major collection, then drops every evaluator object and typed
    /// program object, type by type, reading the malloc counter around each step.
    fn census_exit(&mut self) {
        self.census_snapshot("at exit", false);
        let config = config();
        let before = malloc_bytes();
        self.collect(Collection { check: false, log: 0, threads: config.threads, minor: false, generational: config.generational });
        if counted().is_some() {
            eprintln!("census at exit: the major collection freed {} MiB of malloc", mib(before.saturating_sub(malloc_bytes())));
            self.census_drop_all();
        }
        let peaks = *lock(&PEAKS);
        for (phase, (rss, ms)) in ["between collections", "marking", "sweeping"].iter().zip(peaks) {
            eprintln!("census peak {phase}: {} MiB at {:.1} s", mib(rss), ms as f64 / 1000.0);
        }
    }

    /// Drops every evaluator object, then every typed program object, and prints the
    /// slot and buffer bytes of each type, most first. The heap is unusable afterwards.
    fn census_drop_all(&mut self) {
        let ranges = ranges();
        let mut types = Vec::new();
        for (heap, program, classes, range) in [("evaluator", false, &self.eval, &ranges.eval), ("program", true, &self.program, &ranges.program)] {
            let start = malloc_bytes();
            let mut slots = 0;
            for class in classes.iter().flatten() {
                let slot = class.info.slot as usize;
                let (before, mut count) = (malloc_bytes(), 0usize);
                for &block in &class.blocks {
                    // SAFETY: a block of this class, owned by this thread.
                    let meta = unsafe { range.meta(block) };
                    let base = range.block_base(block);
                    let mut drop_slot = |i: usize| {
                        // SAFETY: an object of this class; nothing reads either heap after this.
                        unsafe { (class.info.drop)(base + i * slot) };
                        count += 1;
                    };
                    if program {
                        (0..meta.bump as usize).for_each(&mut drop_slot);
                    } else {
                        for w in 0..meta.words as usize {
                            // SAFETY: the live bitmap of the block.
                            let mut live = unsafe { *meta.bits.add(w) };
                            while live != 0 {
                                drop_slot(w * 64 + live.trailing_zeros() as usize);
                                live &= live - 1;
                            }
                        }
                    }
                }
                slots += count * slot;
                types.push((heap, class.info.name, count, count * slot, before.saturating_sub(malloc_bytes())));
            }
            eprintln!(
                "census at exit: {heap} objects: {} MiB of slots own {} MiB of buffers; malloc now {} MiB, of which arena chunks {} MiB",
                mib(slots),
                mib(start.saturating_sub(malloc_bytes())),
                mib(malloc_bytes()),
                mib(crate::arena::arena_chunk_bytes())
            );
        }
        types.sort_by_key(|&(_, _, _, slots, buffers)| std::cmp::Reverse(slots + buffers));
        for (heap, name, count, slots, buffers) in types.into_iter().take(25) {
            eprintln!("census   {heap} {name}: {count} objects, {} MiB of slots, {} MiB of buffers", mib(slots), mib(buffers));
        }
    }
}

/// Census: a snapshot once the check loop is done, at about the run's peak.
pub fn census_end_of_check() {
    if on() && config().mode != GcMode::Off {
        with_heap(|heap| heap.census_snapshot("end of check", false));
    }
}

/// Census: call last, after the output is written. Collects, then drops every heap
/// object to attribute buffers; nothing may touch either heap afterwards.
pub fn census_at_exit() {
    if on() && config().mode != GcMode::Off {
        with_heap(|heap| heap.census_exit());
    }
}
