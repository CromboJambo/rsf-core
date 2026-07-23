# rsf-core as Turso Extension — Plan

## Goal

Make `rsf-core` a loadable extension for Turso Database, exposing RSF ranking and functional dependency discovery as SQL functions callable from any SQL client.

```sql
-- Example: discover FDs in a table
SELECT * FROM rsf_fd_dependencies('transactions');

-- Example: get cardinality rank of columns
SELECT column_name, rsf_cardinality(column_name) AS rank 
FROM rsf_column_stats('transactions') ORDER BY 2;
```

## Why This Path

- `rsf-core` is already a pure library (no CLI coupling, no file I/O beyond CSV parsing)
- Turso's extension API (`extensions/core`) expects exactly this: self-contained crates implementing scalar/aggregate/virtual table traits
- Once implemented, any SQL tool (DBeaver, MCP servers, `tursodb --mcp`) gets RSF-aware queries for free
- Mirrors how Turso handles crypto, regexp, and fuzzy extensions

## Architecture

```
rsf-core/src/lib.rs          ← pure types: FieldValue, Table, ranking, FDs
    │
    └── rsf-turso/           ← NEW crate (extension glue)
        ├── Cargo.toml       ← depends on turso_ext + rsf-core
        ├── src/lib.rs       ← ExtensionApi impl
        ├── src/scalar.rs    ← scalar functions: rsf_cardinality(), rsf_rank()
        ├── src/vtab.rs      ← virtual table: rsf_fd_dependencies(table_name)
        └── src/agg.rs       ← aggregate functions (if needed later)
```

## Phases

### Phase 0 — Scaffold (this session)
- [ ] Create `rsf-turso/` crate in the projects workspace
- [ ] Add dependency on `turso_ext` from Turso repo (`extensions/core`)
- [ ] Implement bare-bones `ExtensionApi` trait with a no-op function
- [ ] Verify it compiles and loads via `LOAD 'rsf_turso'`

### Phase 1 — Cardinality scalar function
- [ ] `rsf_cardinality(table_name, column_name) → i64`
- [ ] Accepts table name + column name from SQL context
- [ ] Returns number of distinct non-null values
- [ ] Test via: `SELECT rsf_cardinality('my_table', 'vendor') FROM ...`

### Phase 2 — Column stats virtual table
- [ ] `rsf_column_stats(table_name)` returns a virtual table with columns: name, rank, cardinality, null_pct, unique_pct
- [ ] Implements Turso's VirtualTable trait
- [ ] Test via: `SELECT * FROM rsf_column_stats('transactions') ORDER BY rank`

### Phase 3 — FD discovery virtual table
- [ ] `rsf_fd_dependencies(table_name)` returns a virtual table with columns: source_cols, target_cols, confidence
- [ ] Reuses existing `rsf::fd_discover()` logic from `ranking.rs`
- [ ] Test via: `SELECT * FROM rsf_fd_dependencies('transactions') WHERE confidence > 0.9`

### Phase 4 — Integration tests + documentation
- [ ] Add `.sqltest` files in Turso's conformance suite for RSF extension
- [ ] Document extension API in rsf-core README
- [ ] Update rsf-cli to optionally use the extension for complex queries

## Key Design Decisions

1. **rsf-turso lives alongside rsf-core** (not inside Turso repo) — keeps it as a standalone crate that can be developed independently, then integrated into Turso's extensions/ directory later
2. **No schema changes to existing tables** — extension functions operate on table names passed as strings; the extension introspects via Turso's metadata API
3. **rsf-core stays pure** — no Turso-specific types leak into rsf-core itself; all integration is in the new `rsf-turso` crate

## Files to Create/Modify

| File | Action | Purpose |
|------|--------|---------|
| `projects/rsf-turso/Cargo.toml` | CREATE | Extension crate manifest |
| `projects/rsf-turso/src/lib.rs` | CREATE | Entry point, ExtensionApi impl |
| `projects/rsf-turso/src/scalar.rs` | CREATE | Scalar function implementations |
| `projects/rsf-turso/src/vtab.rs` | CREATE | Virtual table implementations |
| `projects/rsf-core/README.md` | UPDATE | Document extension availability |
| `turso/extensions/` (later) | MOVE | Once stable, move into Turso's extensions/ dir |

## Risks & Mitigations

- **Turso version pinning**: `turso_ext` API may change between versions. Pin to a specific commit/tag in Cargo.toml and note it in docs.
- **Table introspection complexity**: Getting column metadata from within an extension requires using Turso's internal APIs. Start simple (accept table name as string, query via SQL) before optimizing.
- **Performance**: FD discovery is O(n²) on columns. For large tables, add a `LIMIT` or `confidence_threshold` parameter to the virtual table.

## Success Criteria

1. Extension compiles and loads: `.load ./rsf_turso.so` succeeds in tursodb shell
2. Scalar function works: `SELECT rsf_cardinality('test_table', 'col') FROM ...` returns correct count
3. Virtual tables work: `SELECT * FROM rsf_fd_dependencies('test_table')` returns FD results
4. No regressions to existing rsf-core functionality
