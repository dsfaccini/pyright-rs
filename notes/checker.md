# checker notes

The checker round of step 5: `analyzer/checker.ts`, `analyzer/deprecatedSymbols.ts`, and the hooks `run_checker` (`analyzer/source_file.rs`) and `dump_types` (`oracle/types.rs`).

## Signature changes

- `Checker::new` takes `node_info_reader: P<dyn AnalyzerNodeInfoReader>` (was `&dyn`): the walker base stores it (checker.ts:244). `typemap.toml` `[sites]` `Checker.constructor.nodeInfoReader`. Applied to `checker.rs` by hand: `stubgen --install` refuses on main because `type_evaluator.rs` holds a nested function marker (`typeEvaluator.ts:24272`, `apply_loader_actions_to_module_type`) that the stub lacks.

## Shared-file edits

## Needs from others

## Doubts
