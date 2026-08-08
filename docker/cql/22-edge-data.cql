USE edge_cases;

INSERT INTO table_name_of_exactly_forty_eight_characters_max (column_name_of_exactly_forty_eight_characters_ab, another_column_name_of_exactly_forty_eight_chars, k)
    VALUES ('partition-one', 'a value under a very long column name', 1);
INSERT INTO table_name_of_exactly_forty_eight_characters_max (column_name_of_exactly_forty_eight_characters_ab, another_column_name_of_exactly_forty_eight_chars, k)
    VALUES ('partition-two', null, 2);

INSERT INTO t (k, a, b, c) VALUES (1, 'x', 10, true);
INSERT INTO t (k, a, b, c) VALUES (2, 'y', -10, false);
INSERT INTO t (k) VALUES (3);

INSERT INTO "CustomerProfile" ("customerId", "firstName", "HTTPStatusCode", "lastLoginAt", plain_lower)
    VALUES (e0000000-0000-4000-8000-000000000001, 'Ada', 200, '2026-02-27T09:14:02.000Z', 'unquoted column, folded to lower case');
INSERT INTO "CustomerProfile" ("customerId", "firstName", "HTTPStatusCode", "lastLoginAt", plain_lower)
    VALUES (e0000000-0000-4000-8000-000000000002, 'Grace', 503, '2026-03-01T17:45:31.000Z', null);

INSERT INTO reserved_word_columns (id, "select", "from", "where", "table", "token", "order", "index")
    VALUES (1, 'SELECT', 'FROM', 'WHERE', 'TABLE', -9223372036854775808, 1, 2);
INSERT INTO reserved_word_columns (id, "select", "from", "where", "table", "token", "order", "index")
    VALUES (2, 'a value containing the word select', 'from', 'where', 'table', 0, 0, 0);

INSERT INTO cafe_evenements_2026 ("識別子", "名前", "описание", "🔑", "col with spaces", "MiXeD_Ünïcödé")
    VALUES (1, '日本語のテキスト', 'описание на кириллице', '🔐🗝️', 'value under a spaced column name', 'Ünïcödé');
INSERT INTO cafe_evenements_2026 ("識別子", "名前", "описание", "🔑", "col with spaces", "MiXeD_Ünïcödé")
    VALUES (2, '全角文字は幅が二倍', 'ещё одна строка', '🚀', null, 'MiXeD');
INSERT INTO cafe_evenements_2026 ("識別子", "名前")
    VALUES (3, 'مرحبا بالعالم');

INSERT INTO deep_composite_key (tenant_id, region, shard, bucket, ts, seq, kind, payload)
    VALUES ('acme', 'eu-west-1', 0, 0, '2026-03-01T12:00:00.000Z', 1, 'create', 'first');
INSERT INTO deep_composite_key (tenant_id, region, shard, bucket, ts, seq, kind, payload)
    VALUES ('acme', 'eu-west-1', 0, 0, '2026-03-01T12:00:00.000Z', 1, 'update', 'same ts and seq, different kind');
INSERT INTO deep_composite_key (tenant_id, region, shard, bucket, ts, seq, kind, payload)
    VALUES ('acme', 'eu-west-1', 0, 0, '2026-03-01T11:00:00.000Z', 2, 'create', 'earlier ts sorts last');
INSERT INTO deep_composite_key (tenant_id, region, shard, bucket, ts, seq, kind, payload)
    VALUES ('acme', 'eu-west-1', 1, 0, '2026-03-01T12:00:00.000Z', 1, 'create', 'different shard, different partition');
INSERT INTO deep_composite_key (tenant_id, region, shard, bucket, ts, seq, kind, payload)
    VALUES ('acme', 'us-east-1', 0, 7, '2026-03-01T12:00:00.000Z', 9223372036854775807, 'delete', 'max seq');
INSERT INTO deep_composite_key (tenant_id, region, shard, bucket, ts, seq, kind, payload)
    VALUES ('', '', 0, -32768, '1970-01-01T00:00:00.000Z', -9223372036854775808, '', 'empty strings are a legal partition key');

INSERT INTO partition_key_only (only_key) VALUES ('alpha');
INSERT INTO partition_key_only (only_key) VALUES ('beta');
INSERT INTO partition_key_only (only_key) VALUES ('   leading and trailing spaces   ');
-- No empty-string row here: a single-component partition key rejects '' with
-- "Key may not be empty", while a composite one accepts empty components —
-- see the deep_composite_key row above.
