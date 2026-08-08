-- One table per type family, so a rendering bug points at a type rather than
-- at a table. Names stay boring here on purpose: `edge_cases` owns the
-- identifier torture, this keyspace owns the value torture.

USE typezoo;

CREATE TYPE IF NOT EXISTS address (
    street       text,
    city         text,
    postal_code  text,
    country_code text
);

CREATE TYPE IF NOT EXISTS geo_point (
    lat double,
    lon double
);

-- Nested UDT: `contact` embeds two other UDTs plus a frozen collection.
CREATE TYPE IF NOT EXISTS contact (
    kind            text,
    primary_address frozen<address>,
    location        frozen<geo_point>,
    phone_numbers   frozen<list<text>>
);

CREATE TABLE IF NOT EXISTS all_scalar_types (
    id            int,
    col_ascii     ascii,
    col_bigint    bigint,
    col_blob      blob,
    col_boolean   boolean,
    col_date      date,
    col_decimal   decimal,
    col_double    double,
    col_duration  duration,
    col_float     float,
    col_inet      inet,
    col_int       int,
    col_smallint  smallint,
    col_text      text,
    col_time      time,
    col_timestamp timestamp,
    col_timeuuid  timeuuid,
    col_tinyint   tinyint,
    col_uuid      uuid,
    col_varchar   varchar,
    col_varint    varint,
    PRIMARY KEY ((id))
) WITH comment = 'Every CQL scalar type, one column each.';

CREATE TABLE IF NOT EXISTS collection_types (
    id                 int,
    list_of_text       list<text>,
    list_of_int        list<int>,
    set_of_uuid        set<uuid>,
    set_of_timestamp   set<timestamp>,
    map_text_int       map<text, int>,
    map_uuid_text      map<uuid, text>,
    frozen_list        frozen<list<text>>,
    frozen_set         frozen<set<int>>,
    frozen_map         frozen<map<text, text>>,
    list_of_frozen_map list<frozen<map<text, int>>>,
    map_of_frozen_set  map<text, frozen<set<text>>>,
    nested_frozen      frozen<map<text, frozen<list<frozen<set<int>>>>>>,
    PRIMARY KEY ((id))
);

CREATE TABLE IF NOT EXISTS udt_types (
    id        int,
    home      frozen<address>,
    work      address,
    contacts  list<frozen<contact>>,
    by_label  map<text, frozen<address>>,
    waypoints frozen<list<frozen<geo_point>>>,
    PRIMARY KEY ((id))
) WITH comment = 'Frozen and non-frozen UDTs, UDTs nested in collections.';

CREATE TABLE IF NOT EXISTS tuple_types (
    id           int,
    pair         tuple<int, text>,
    triple       tuple<text, timestamp, double>,
    nested_tuple tuple<int, frozen<tuple<text, boolean>>>,
    tuple_list   list<frozen<tuple<text, int>>>,
    PRIMARY KEY ((id))
);

-- Cassandra 5.0 only. `embedding_384` is filled by seed.sh rather than being
-- a 384-element literal checked into this file.
CREATE TABLE IF NOT EXISTS vector_types (
    id            int,
    label         text,
    embedding_3   vector<float, 3>,
    embedding_384 vector<float, 384>,
    PRIMARY KEY ((id))
);

CREATE TABLE IF NOT EXISTS static_columns (
    device_id     text,
    reading_at    timestamp,
    device_model  text static,
    firmware      text static,
    calibrated_at timestamp static,
    temperature_c float,
    humidity_pct  float,
    PRIMARY KEY ((device_id), reading_at)
) WITH CLUSTERING ORDER BY (reading_at DESC)
  AND comment = 'Static columns repeat per partition, not per row.';

-- Cassandra stores an empty collection as null, and an empty text as a
-- zero-length value that is not null. A grid that renders both as blank is
-- lying about one of them.
CREATE TABLE IF NOT EXISTS nulls_and_empties (
    id         int,
    note       text,
    maybe_text text,
    maybe_blob blob,
    maybe_int  int,
    maybe_list list<text>,
    maybe_map  map<text, text>,
    maybe_udt  frozen<address>,
    PRIMARY KEY ((id))
);
