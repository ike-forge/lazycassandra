-- Identifier and shape torture. Nothing here is realistic; everything here is
-- legal. 48 characters is Cassandra's hard limit for keyspace, table and
-- column names, so the long names below sit exactly on it.

USE edge_cases;

CREATE TABLE IF NOT EXISTS table_name_of_exactly_forty_eight_characters_max (
    column_name_of_exactly_forty_eight_characters_ab text,
    another_column_name_of_exactly_forty_eight_chars text,
    k int,
    PRIMARY KEY ((column_name_of_exactly_forty_eight_characters_ab))
) WITH comment = 'Table and first two column names are exactly 48 characters.';

-- Single-character everything.
CREATE TABLE IF NOT EXISTS t (
    k int,
    a text,
    b int,
    c boolean,
    PRIMARY KEY ((k))
);

-- Quoted identifiers preserve case; unquoted ones are folded to lower case.
-- A tool that re-quotes wrongly will not find this table.
CREATE TABLE IF NOT EXISTS "CustomerProfile" (
    "customerId"     uuid,
    "firstName"      text,
    "HTTPStatusCode" int,
    "lastLoginAt"    timestamp,
    plain_lower      text,
    PRIMARY KEY (("customerId"))
);

-- CQL reserved words are legal column names once quoted.
CREATE TABLE IF NOT EXISTS reserved_word_columns (
    id       int,
    "select" text,
    "from"   text,
    "where"  text,
    "table"  text,
    "token"  bigint,
    "order"  int,
    "index"  int,
    PRIMARY KEY ((id))
);

-- Column names accept any quoted identifier: non-ASCII, double-width CJK,
-- spaces, a bare emoji. Keyspace and table names do not — Cassandra rejects
-- anything outside [A-Za-z0-9_] there, because the name becomes a directory
-- on disk. So the table below is ASCII and only its columns are hostile.
CREATE TABLE IF NOT EXISTS cafe_evenements_2026 (
    "識別子"        int,
    "名前"          text,
    "описание"      text,
    "🔑"            text,
    "col with spaces" text,
    "MiXeD_Ünïcödé" text,
    PRIMARY KEY (("識別子"))
);

-- Four partition-key components, three clustering columns, mixed ordering.
CREATE TABLE IF NOT EXISTS deep_composite_key (
    tenant_id text,
    region    text,
    shard     int,
    bucket    smallint,
    ts        timestamp,
    seq       bigint,
    kind      text,
    payload   text,
    PRIMARY KEY ((tenant_id, region, shard, bucket), ts, seq, kind)
) WITH CLUSTERING ORDER BY (ts DESC, seq ASC, kind DESC);

-- Degenerate shape: a table with nothing but its partition key.
CREATE TABLE IF NOT EXISTS partition_key_only (
    only_key text,
    PRIMARY KEY ((only_key))
);

-- Values chosen to break a terminal rather than a parser: raw ANSI escape
-- sequences, C0 control characters, RTL and combining marks, double-width
-- CJK, a 100 KB text and a 64 KB blob. Written by seed.sh, which can emit the
-- escape bytes explicitly.
CREATE TABLE IF NOT EXISTS hostile_values (
    id      int,
    label   text,
    value   text,
    payload blob,
    PRIMARY KEY ((id))
);

-- 100 columns: the horizontal-overflow fixture. Names alternate between
-- terse and 48-character so column-width fitting has to cope with both.
CREATE TABLE IF NOT EXISTS wide_row_one_hundred_columns (
    k int,
    c001                                             text,
    c002                                             int,
    c003                                             bigint,
    c004                                             boolean,
    wide_col_005_with_a_fairly_long_descriptive_name double,
    c006                                             timestamp,
    c007                                             uuid,
    c008                                             blob,
    c009                                             decimal,
    wide_col_010_with_a_fairly_long_descriptive_name float,
    c011                                             text,
    c012                                             int,
    c013                                             bigint,
    c014                                             boolean,
    wide_col_015_with_a_fairly_long_descriptive_name double,
    c016                                             timestamp,
    c017                                             uuid,
    c018                                             blob,
    c019                                             decimal,
    wide_col_020_with_a_fairly_long_descriptive_name float,
    c021                                             text,
    c022                                             int,
    c023                                             bigint,
    c024                                             boolean,
    wide_col_025_with_a_fairly_long_descriptive_name double,
    c026                                             timestamp,
    c027                                             uuid,
    c028                                             blob,
    c029                                             decimal,
    wide_col_030_with_a_fairly_long_descriptive_name float,
    c031                                             text,
    c032                                             int,
    c033                                             bigint,
    c034                                             boolean,
    wide_col_035_with_a_fairly_long_descriptive_name double,
    c036                                             timestamp,
    c037                                             uuid,
    c038                                             blob,
    c039                                             decimal,
    wide_col_040_with_a_fairly_long_descriptive_name float,
    c041                                             text,
    c042                                             int,
    c043                                             bigint,
    c044                                             boolean,
    wide_col_045_with_a_fairly_long_descriptive_name double,
    c046                                             timestamp,
    c047                                             uuid,
    c048                                             blob,
    c049                                             decimal,
    wide_col_050_with_a_fairly_long_descriptive_name float,
    c051                                             text,
    c052                                             int,
    c053                                             bigint,
    c054                                             boolean,
    wide_col_055_with_a_fairly_long_descriptive_name double,
    c056                                             timestamp,
    c057                                             uuid,
    c058                                             blob,
    c059                                             decimal,
    wide_col_060_with_a_fairly_long_descriptive_name float,
    c061                                             text,
    c062                                             int,
    c063                                             bigint,
    c064                                             boolean,
    wide_col_065_with_a_fairly_long_descriptive_name double,
    c066                                             timestamp,
    c067                                             uuid,
    c068                                             blob,
    c069                                             decimal,
    wide_col_070_with_a_fairly_long_descriptive_name float,
    c071                                             text,
    c072                                             int,
    c073                                             bigint,
    c074                                             boolean,
    wide_col_075_with_a_fairly_long_descriptive_name double,
    c076                                             timestamp,
    c077                                             uuid,
    c078                                             blob,
    c079                                             decimal,
    wide_col_080_with_a_fairly_long_descriptive_name float,
    c081                                             text,
    c082                                             int,
    c083                                             bigint,
    c084                                             boolean,
    wide_col_085_with_a_fairly_long_descriptive_name double,
    c086                                             timestamp,
    c087                                             uuid,
    c088                                             blob,
    c089                                             decimal,
    wide_col_090_with_a_fairly_long_descriptive_name float,
    c091                                             text,
    c092                                             int,
    c093                                             bigint,
    c094                                             boolean,
    wide_col_095_with_a_fairly_long_descriptive_name double,
    c096                                             timestamp,
    c097                                             uuid,
    c098                                             blob,
    c099                                             decimal,
    PRIMARY KEY ((k))
) WITH comment = '99 value columns plus the partition key.';
