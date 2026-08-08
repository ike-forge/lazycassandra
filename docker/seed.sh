#!/usr/bin/env bash
#
# Seeds the fixture keyspaces. Runs inside the `seed` service of
# docker/compose.yaml, which waits for the node to report healthy.
#
# Idempotent: all DDL is IF NOT EXISTS and every INSERT is an upsert with a
# literal primary key, so re-running against a populated volume is a no-op.
#
# Deterministic: no now(), no uuid(), no $RANDOM, no wall clock. Two runs
# against empty volumes produce identical data.
#
# Three things are generated here rather than checked into cql/:
#   - the 10 000-row wide partition, which would be a 2 MB file
#   - the 384-dimension vector, which would be a 3 KB single-line literal
#   - the hostile values, which need raw ESC and C0 bytes that a text editor
#     silently mangles

set -euo pipefail

HOST="${CQLSH_HOST:-cassandra}"
PORT="${CQLSH_PORT:-9042}"
CQL_DIR="${CQL_DIR:-/fixture/cql}"

AUDIT_DAY='2026-03-01'
AUDIT_ROWS=10000
AUDIT_BATCH=100
VECTOR_DIMS=384

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cql_file() {
    cqlsh "$HOST" "$PORT" --request-timeout=120 -f "$1"
}

step() {
    printf '\n=== %s\n' "$1"
}

step "Applying schema and hand-written data from ${CQL_DIR}"
for f in "$CQL_DIR"/*.cql; do
    printf '  %s\n' "$(basename "$f")"
    cql_file "$f"
done

# Counters are the one thing here that is not an upsert: `SET x = x + n` is an
# increment, so replaying it multiplies the totals. There is no LWT for counter
# tables either, so the guard has to be an explicit read.
step "Seeding counters (shop.product_view_counts)"
echo "SELECT product_id FROM shop.product_view_counts LIMIT 1;" > "$WORK/counter-probe.cql"
if cql_file "$WORK/counter-probe.cql" | grep -q '(0 rows)'; then
    cat > "$WORK/counters.cql" <<'EOF'
UPDATE shop.product_view_counts SET views = views + 18452, add_to_cart = add_to_cart + 903
    WHERE product_id = b0000000-0000-4000-8000-000000000001 AND day = '2026-03-01';
UPDATE shop.product_view_counts SET views = views + 17110, add_to_cart = add_to_cart + 874
    WHERE product_id = b0000000-0000-4000-8000-000000000001 AND day = '2026-02-28';
UPDATE shop.product_view_counts SET views = views + 3, add_to_cart = add_to_cart + 0
    WHERE product_id = b0000000-0000-4000-8000-000000000006 AND day = '2026-03-01';
EOF
    cql_file "$WORK/counters.cql"
else
    printf '  already populated, skipping\n'
fi

step "Generating the ${AUDIT_ROWS}-row wide partition (shop.audit_log_by_day, ${AUDIT_DAY})"
awk -v rows="$AUDIT_ROWS" -v batch="$AUDIT_BATCH" -v day="$AUDIT_DAY" '
BEGIN {
    q = sprintf("%c", 39)
    na = split("ada.lovelace@example.com grace.hopper@example.com alan.turing@example.com system scheduler", actors, " ")
    nc = split("LOGIN LOGOUT ORDER_PLACED ORDER_SHIPPED PAYMENT_CAPTURED PRICE_CHANGED STOCK_ADJUSTED PERMISSION_GRANTED", actions, " ")
    for (i = 0; i < rows; i++) {
        if (i % batch == 0) print "BEGIN UNLOGGED BATCH"
        sec = i * 8
        ts = sprintf("%sT%02d:%02d:%02dZ", day, int(sec / 3600), int((sec % 3600) / 60), sec % 60)
        printf "  INSERT INTO shop.audit_log_by_day (day, logged_at, event_id, actor, action, target, detail) VALUES (%s%s%s, %s%s%s, c1000000-0000-4000-8000-%012x, %s%s%s, %s%s%s, %sentity:%06d%s, %srow %d of %d in the wide partition%s);\n", \
            q, day, q, q, ts, q, i, q, actors[(i % na) + 1], q, q, actions[(i % nc) + 1], q, q, i, q, q, i, rows, q
        if (i % batch == batch - 1 || i == rows - 1) print "APPLY BATCH;"
    }
}' > "$WORK/wide-partition.cql"
cql_file "$WORK/wide-partition.cql"

step "Generating the ${VECTOR_DIMS}-dimension vector (typezoo.vector_types)"
awk -v dims="$VECTOR_DIMS" '
BEGIN {
    q = sprintf("%c", 39)
    printf "INSERT INTO typezoo.vector_types (id, label, embedding_3, embedding_384) VALUES (3, %sgenerated %d-dim embedding%s, [0.25, 0.5, 0.75], [", q, dims, q
    for (i = 0; i < dims; i++) {
        if (i) printf ", "
        printf "%.6f", (i % 97) / 97.0
    }
    print "]);"
}' > "$WORK/vector.cql"
cql_file "$WORK/vector.cql"

step "Generating hostile values (edge_cases.hostile_values)"
H="$WORK/hostile.cql"
: > "$H"
printf "USE edge_cases;\n" >> "$H"
# Raw ESC bytes. A grid that writes cell contents straight to the terminal
# will repaint in red and then clear the screen when this row scrolls past.
printf "INSERT INTO hostile_values (id, label, value) VALUES (1, 'ANSI escape sequences', 'sgr[\033[31mRED\033[0m] erase[\033[2J] home[\033[H] title[\033]0;pwned\007]');\n" >> "$H"
printf "INSERT INTO hostile_values (id, label, value) VALUES (2, 'C0 control characters', 'tab[\t] lf[\n] cr[\r] bs[\b] bel[\a] vt[\v] ff[\f]');\n" >> "$H"
printf "INSERT INTO hostile_values (id, label, value) VALUES (3, 'Double-width CJK', '全角文字は幅が二倍 中文測試 한국어');\n" >> "$H"
printf "INSERT INTO hostile_values (id, label, value) VALUES (4, 'Bidirectional text', 'before مرحبا بالعالم after, override[‮REVERSED‬]');\n" >> "$H"
printf "INSERT INTO hostile_values (id, label, value) VALUES (5, 'Combining and zero-width', 'decomposed[é] stacked[á̂̃̄] zwsp[​] zwj[‍] family[👩‍👩‍👧] flag[🏴󠁧󠁢󠁳󠁣󠁴󠁿]');\n" >> "$H"
printf "INSERT INTO hostile_values (id, label, value) VALUES (8, 'Quote and backslash escaping', 'it''s a single quote, a backslash \\\\ and a semicolon ; mid-literal');\n" >> "$H"
awk '
BEGIN {
    q = sprintf("%c", 39)
    chunk = "The quick brown fox jumps over the lazy dog. 0123456789. "
    s = ""
    for (i = 0; i < 1800; i++) s = s chunk
    printf "INSERT INTO hostile_values (id, label, value) VALUES (6, %s100 KB of text in one cell%s, %s%s%s);\n", q, q, q, s, q
    printf "INSERT INTO hostile_values (id, label, payload) VALUES (7, %s64 KB blob%s, 0x", q, q
    for (i = 0; i < 65536; i++) printf "%02x", i % 256
    print ");"
}' >> "$H"
cql_file "$H"

step "Generating the 100-column row (edge_cases.wide_row_one_hundred_columns)"
awk '
BEGIN {
    q = sprintf("%c", 39)
    split("text int bigint boolean double timestamp uuid blob decimal float", t, " ")
    cols = "k"; dense = "1"; sparse = "2"
    for (i = 1; i <= 99; i++) {
        ty = t[((i - 1) % 10) + 1]
        name = (i % 5 == 0) ? sprintf("wide_col_%03d_with_a_fairly_long_descriptive_name", i) : sprintf("c%03d", i)
        if (ty == "text")           v = sprintf("%sv%03d%s", q, i, q)
        else if (ty == "int")       v = sprintf("%d", i)
        else if (ty == "bigint")    v = sprintf("%d", i * 1000000000)
        else if (ty == "boolean")   v = (i % 2 == 0) ? "true" : "false"
        else if (ty == "double")    v = sprintf("%.6f", i / 7.0)
        else if (ty == "timestamp") v = sprintf("%s2026-03-01T%02d:00:00Z%s", q, i % 24, q)
        else if (ty == "uuid")      v = sprintf("f0000000-0000-4000-8000-%012x", i)
        else if (ty == "blob")      v = sprintf("0x%02x%02x", i, 255 - i)
        else if (ty == "decimal")   v = sprintf("%d.%02d", i, i % 100)
        else                        v = sprintf("%.3f", i / 3.0)
        cols = cols ", " name
        dense = dense ", " v
        # Every tenth column populated: a sparse row next to a dense one.
        sparse = sparse ", " ((i % 10 == 0) ? v : "null")
    }
    printf "INSERT INTO edge_cases.wide_row_one_hundred_columns (%s) VALUES (%s);\n", cols, dense
    printf "INSERT INTO edge_cases.wide_row_one_hundred_columns (%s) VALUES (%s);\n", cols, sparse
}' > "$WORK/wide-row.cql"
cql_file "$WORK/wide-row.cql"

step "Summary"
# Via -f, not -e: cqlsh's -e splitter mis-handles statements that span lines
# and reports a spurious trailing syntax error.
cat > "$WORK/summary.cql" <<EOF
SELECT keyspace_name, table_name FROM system_schema.tables
  WHERE keyspace_name IN ('shop', 'typezoo', 'edge_cases');
SELECT keyspace_name, view_name FROM system_schema.views
  WHERE keyspace_name IN ('shop', 'typezoo', 'edge_cases');
SELECT count(*) AS audit_rows FROM shop.audit_log_by_day WHERE day = '${AUDIT_DAY}';
EOF
cql_file "$WORK/summary.cql"

printf '\nSeeding complete.\n'
