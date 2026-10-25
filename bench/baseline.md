# Baseline: pyright 1.1.414 on pydantic-ai

Measured 2026-10-25 on devbox (Ryzen 7 7700, 64 GB) with `bench/baseline.sh`.
pydantic-ai `ead5e549b`, environment synced like CI's lint job: uv-managed Python 3.13,
`uv sync --frozen --all-extras --no-extra mcp-tasks --all-packages --group lint`. 2,046 files checked.

| run | wall | user CPU | peak RSS | errors |
|---|---|---|---|---|
| Node default heap | 2:51 | 293 s | 4.2 GB | 0 |
| `--max-old-space-size=16384 --verbose` | 1:59 | 161 s | 10.4 GB | 0 |
| `--max-old-space-size=16384 --outputjson` (reference) | 1:59 | 164 s | 9.3 GB | 0 |

- The 16 GB `--verbose` run printed no "Emptying type cache", so `pydantic-ai-reference.json` is a valid reference.
- Heap size changes pyright's output. At the pyright `f2e1a53` pin with Python 3.14, the default heap gave 222 errors and the 16 GB heap gave 224; the two extra were "Unnecessary `# pyright: ignore`" in `providers/voyageai.py`.
- pydantic-ai checks clean, so its diagnostics alone cannot prove the port correct. An empty output also matches. Correctness on pydantic-ai needs the per-expression type dump from step 1 of `PLAN.md`.
