# Acceptance test coverage registry

This registry is the source of truth for automated claims in `MATRIX.md`. Test data uses only
synthetic values; no cookie or secret plaintext is written to logs or retained as an artifact.

| Test ID | Exact test | Scope |
|---|---|---|
| `ACC-NMH-005` | `tests/acceptance/native_handshake.py::main` — `fursoy-acceptance-v7-floor-` block | Real release host v7 activation persists lease floor and rejects a later v6 downgrade |
| `ACC-NMH-001` | `tests/acceptance/native_handshake.py::main` — first temporary-directory block | Real release host process: v6 request correlation and advertised capabilities |
| `ACC-NMH-002` | `tests/acceptance/native_handshake.py::main` — `fursoy-acceptance-origin-` block | Real release host process rejects an unauthorized extension origin |
| `ACC-NMH-003` | `tests/acceptance/native_handshake.py::main` — `fursoy-acceptance-version-` block | Real release host process rejects protocol v5 |
| `ACC-NMH-004` | `tests/acceptance/native_handshake.py::main` — `fursoy-acceptance-profiles-` block | Real release host process isolates profile config and rejects cross-profile recovery claim |
| `CHUNK-EXT-001` | `extension/tests/cookie-chunks.test.mjs` | Production extension chunker and ordered assembler with >1 MiB aggregate, limits, mismatch, and clean retry |
| `CHUNK-HOST-001` | `dispatcher::tests::cookie_chunking_carries_an_aggregate_larger_than_one_mib_in_bounded_ordered_frames` | Production native-host chunker preserves order and target-sized frames |
| `CHUNK-HOST-002` | `dispatcher::tests::snapshot_handler_rejects_declared_cookie_count_above_the_production_limit` | Production snapshot handler rejects the cookie-count bound without mutating progress |
| `FAIL-ATOMIC-001` | `atomic_file::tests::failure_before_replace_preserves_the_previous_target` | Deterministic pre-commit failure, including temporary-file cleanup |
| `FAIL-ATOMIC-002` | `atomic_file::tests::failure_after_replace_reports_error_with_the_new_target_committed` | Deterministic post-commit error documents the current ambiguous durable outcome |
| `FAIL-LEASE-001` | `lease::metadata::tests::injected_lease_persistence_failure_does_not_replace_the_previous_metadata` | Lease persistence uses the production serializer/store and fails before atomic commit |
| `FAIL-AUDIT-001` | `audit::tests::injected_failure_before_append_leaves_the_chain_unchanged` | Audit append pre-commit failure leaves the durable chain unchanged |
| `FAIL-AUDIT-002` | `audit::tests::injected_failure_after_append_is_recoverable_but_reports_an_error` | Audit append post-commit error exposes committed-but-reported-failed and exercises strict reopen |
| `FAIL-PROTOCOL-001` | `protocol::framing::tests::injected_response_boundary_failure_writes_no_frame_bytes` | Response failure before production framing writes nothing |
| `FAIL-PROTOCOL-002` | `protocol::framing::tests::writer_failure_after_length_prefix_leaves_a_detectably_truncated_frame` | Scripted writer fails after the native-message length prefix |
| `CRASH-VAULT-001` | `vault::store::tests::staged_delete_rolls_back_or_finishes_from_config_membership` | Production startup recovery resolves both staged-delete outcomes from config membership |
| `RESTART-DECISION-001` | `extension/tests/connection-readiness.test.mjs` | Pure connection-readiness state requires a fresh ACK after disconnect/reconnect |
| `TXN-VAULT-001` | `vault::store::tests::prepared_bytes_are_the_exact_bytes_committed_to_the_vault` | Exact prepared encrypted bytes are the bytes present at the authoritative vault path after commit |
| `TXN-RECOVERY-001` | `operation::coordinator::tests::durability_unknown_at_vault_replace_is_classified_as_committed` | Post-replace error is resolved from the exact target object rather than the returned I/O error |
| `TXN-RECOVERY-002` | `operation::coordinator::tests::vault_commit_with_lagging_journal_is_recovered_after_restart` | Target vault commit with a lagging durability-unknown journal is recovered on reopen |
| `TXN-LEASE-001` | `dispatcher::tests::journal_completed_but_lease_persist_failed_is_repaired_on_restart` | Terminal journal authority repairs the bound lease v1 projection after injected lease persistence failure |
| `TXN-GATE-001` | `dispatcher::tests::nonterminal_committed_operation_is_recovered_degraded_and_blocks_new_lease` | Startup preserves committed incomplete work, degrades the lease projection, and denies new inject/lease work |
| `TXN-KEY-001` | `dispatcher::tests::missing_or_corrupted_snapshot_key_with_a_journal_fails_closed` | Missing or corrupt DPAPI snapshot-integrity key with durable journals prevents host startup |
| `V6-COMPAT-001` | `dispatcher::tests::v6_handshake_shape_does_not_expose_internal_operation_journal_fields` | Internal journal integration does not change protocol v6 handshake payload shape |
| `V7-OP-001` | `operation::coordinator::tests::v7_attempt_is_host_issued_duplicate_safe_and_payload_bound` | Host-issued operation identity, monotonic sequence, retry lookup and payload conflict rejection |
| `V7-LEASE-001` | `lease::metadata::tests::stable_v1_migrates_atomically_and_transitional_v1_fails_closed` | Lease v1→v2 migration and transitional-state fail-closed projection |
| `V7-FLOOR-001` | `lease::metadata::tests::protocol_floor_is_monotonic` | Durable protocol floor and operation high-water never roll back |
| `V7-EXT-001` | `extension/tests/operation-coordinator.test.mjs` | Non-secret service-worker operation restore, status query and binding rejection |
| `V7-REMOVE-001` | `extension/tests/guarded-removal.test.mjs` | Production guarded-removal plan aborts pre-removal rotation, mid-batch mutation and unexpected post-partial state |
| `AUDIT-IDEMP-001` | `audit::tests::record_once_deduplicates_after_append_and_restart` | Deterministic audit event append is idempotent across retry and restart |

The deterministic failpoint enum in `native-host/src/test_support.rs` is compiled only for Rust
unit tests. The extension chunk seam has no injected global state and is exercised directly from
the built module. Neither mechanism changes release behavior or protocol/schema versions.
