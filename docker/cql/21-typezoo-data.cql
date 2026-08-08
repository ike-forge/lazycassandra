USE typezoo;

-- Row 1: ordinary values. Row 2: the extremes of each type's range, which is
-- where column-width heuristics tend to fall over.
INSERT INTO all_scalar_types (id, col_ascii, col_bigint, col_blob, col_boolean, col_date, col_decimal, col_double, col_duration, col_float, col_inet, col_int, col_smallint, col_text, col_time, col_timestamp, col_timeuuid, col_tinyint, col_uuid, col_varchar, col_varint)
    VALUES (1, 'plain-ascii', 4294967296, 0x00010203ff, true, '2026-03-14', 3.14159265358979323846, 2.718281828459045, 89h4m48s, 1.5, '192.168.1.1', 42, 512, 'ordinary text', '13:30:54.234000000', '2026-03-14T13:30:54.234Z', f3a54e00-2b00-11f0-8000-0242ac120010, 7, d0000000-0000-4000-8000-000000000001, 'ordinary varchar', 123456789012345678901234567890);

INSERT INTO all_scalar_types (id, col_ascii, col_bigint, col_blob, col_boolean, col_date, col_decimal, col_double, col_duration, col_float, col_inet, col_int, col_smallint, col_text, col_time, col_timestamp, col_timeuuid, col_tinyint, col_uuid, col_varchar, col_varint)
    VALUES (2, '', -9223372036854775808, 0x, false, '1970-01-01', -0.000000000000000000001, -1.7976931348623157E308, -P10000DT23H59M59S, -3.4028235E38, '2001:db8:85a3::8a2e:370:7334', -2147483648, -32768, '', '00:00:00.000000000', '1970-01-01T00:00:00.000Z', f3a54e00-2b00-11f0-8000-0242ac120011, -128, d0000000-0000-4000-8000-000000000002, '', -99999999999999999999999999999999999999);

INSERT INTO all_scalar_types (id, col_ascii, col_bigint, col_blob, col_boolean, col_date, col_decimal, col_double, col_duration, col_float, col_inet, col_int, col_smallint, col_text, col_time, col_timestamp, col_timeuuid, col_tinyint, col_uuid, col_varchar, col_varint)
    VALUES (3, 'max-ish', 9223372036854775807, 0xdeadbeefcafebabe, true, '9999-12-31', 99999999999999999999.99999999999999999999, 1.7976931348623157E308, P3Y6M4DT12H30M5S, 3.4028235E38, '255.255.255.255', 2147483647, 32767, 'the quick brown fox jumps over the lazy dog', '23:59:59.999999999', '9999-12-31T23:59:59.999Z', f3a54e00-2b00-11f0-8000-0242ac120012, 127, d0000000-0000-4000-8000-000000000003, 'unicode: naïve café résumé', 99999999999999999999999999999999999999);

INSERT INTO collection_types (id, list_of_text, list_of_int, set_of_uuid, set_of_timestamp, map_text_int, map_uuid_text, frozen_list, frozen_set, frozen_map, list_of_frozen_map, map_of_frozen_set, nested_frozen)
    VALUES (1,
        ['alpha', 'beta', 'gamma'],
        [1, 2, 3, 5, 8, 13, 21],
        {d0000000-0000-4000-8000-000000000001, d0000000-0000-4000-8000-000000000002},
        {'2026-01-01T00:00:00.000Z', '2026-06-30T12:00:00.000Z'},
        {'reads': 1204, 'writes': 87},
        {d0000000-0000-4000-8000-000000000001: 'primary'},
        ['frozen', 'list'],
        {1, 1, 2, 3},
        {'k': 'v'},
        [{'a': 1}, {'b': 2, 'c': 3}],
        {'evens': {'2', '4'}, 'odds': {'1', '3', '5'}},
        {'outer': [{1, 2}, {3}]});

-- Empty collections: written as empty, read back as null.
INSERT INTO collection_types (id, list_of_text, list_of_int, set_of_uuid, map_text_int, frozen_list, frozen_set, frozen_map)
    VALUES (2, [], [], {}, {}, [], {}, {});

-- A single collection large enough to overflow any sane cell width.
INSERT INTO collection_types (id, list_of_text, map_text_int)
    VALUES (3,
        ['one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven', 'twelve', 'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen', 'twenty'],
        {'a': 1, 'b': 2, 'c': 3, 'd': 4, 'e': 5, 'f': 6, 'g': 7, 'h': 8, 'i': 9, 'j': 10, 'k': 11, 'l': 12, 'm': 13, 'n': 14, 'o': 15, 'p': 16, 'q': 17, 'r': 18, 's': 19, 't': 20});

INSERT INTO udt_types (id, home, work, contacts, by_label, waypoints)
    VALUES (1,
        {street: '1 Rue de Rivoli', city: 'Paris', postal_code: '75001', country_code: 'FR'},
        {street: 'Bahnhofstrasse 12', city: 'Zürich', postal_code: '8001', country_code: 'CH'},
        [{kind: 'work', primary_address: {street: 'Bahnhofstrasse 12', city: 'Zürich', postal_code: '8001', country_code: 'CH'}, location: {lat: 47.3769, lon: 8.5417}, phone_numbers: ['+41 44 000 00 00', '+41 79 000 00 00']}],
        {'billing': {street: '1 Rue de Rivoli', city: 'Paris', postal_code: '75001', country_code: 'FR'}},
        [{lat: 47.3769, lon: 8.5417}, {lat: 48.8566, lon: 2.3522}, {lat: -33.8688, lon: 151.2093}]);

-- Partially populated UDT: absent fields are null inside the value.
INSERT INTO udt_types (id, home, contacts)
    VALUES (2, {city: 'Reykjavík', country_code: 'IS'}, []);

INSERT INTO tuple_types (id, pair, triple, nested_tuple, tuple_list)
    VALUES (1, (7, 'seven'), ('label', '2026-03-14T13:30:54.234Z', 1.25), (1, ('inner', true)), [('a', 1), ('b', 2)]);

-- Trailing tuple element left null.
INSERT INTO tuple_types (id, pair, triple)
    VALUES (2, (0, null), (null, null, 0.0));

INSERT INTO vector_types (id, label, embedding_3)
    VALUES (1, 'unit x', [1.0, 0.0, 0.0]);
INSERT INTO vector_types (id, label, embedding_3)
    VALUES (2, 'negatives', [-0.5, -1.25, 3.5]);

INSERT INTO static_columns (device_id, device_model, firmware, calibrated_at)
    VALUES ('probe-01', 'AcmeTherm 3000', '2.4.1', '2026-01-15T08:00:00.000Z');
INSERT INTO static_columns (device_id, reading_at, temperature_c, humidity_pct)
    VALUES ('probe-01', '2026-03-01T12:00:00.000Z', 21.4, 47.5);
INSERT INTO static_columns (device_id, reading_at, temperature_c, humidity_pct)
    VALUES ('probe-01', '2026-03-01T11:00:00.000Z', 21.1, 48.0);
INSERT INTO static_columns (device_id, reading_at, temperature_c, humidity_pct)
    VALUES ('probe-01', '2026-03-01T10:00:00.000Z', 20.8, 48.25);
INSERT INTO static_columns (device_id, device_model, firmware, calibrated_at)
    VALUES ('probe-02', 'AcmeTherm 3000', '2.3.0', '2025-11-02T08:00:00.000Z');
INSERT INTO static_columns (device_id, reading_at, temperature_c, humidity_pct)
    VALUES ('probe-02', '2026-03-01T12:00:00.000Z', 19.0, 51.0);
-- A partition holding only its static columns: no clustering rows at all.
INSERT INTO static_columns (device_id, device_model, firmware, calibrated_at)
    VALUES ('probe-03', 'AcmeTherm 1000', '1.0.0', '2024-04-01T08:00:00.000Z');

INSERT INTO nulls_and_empties (id, note, maybe_text, maybe_blob, maybe_int, maybe_list, maybe_map, maybe_udt)
    VALUES (1, 'everything populated', 'present', 0x01, 1, ['x'], {'k': 'v'}, {street: 'a', city: 'b', postal_code: 'c', country_code: 'DE'});
INSERT INTO nulls_and_empties (id, note)
    VALUES (2, 'everything else absent (never written)');
INSERT INTO nulls_and_empties (id, note, maybe_text, maybe_blob, maybe_int, maybe_list, maybe_map, maybe_udt)
    VALUES (3, 'everything else explicitly null', null, null, null, null, null, null);
INSERT INTO nulls_and_empties (id, note, maybe_text, maybe_blob, maybe_list, maybe_map)
    VALUES (4, 'empty, not null', '', 0x, [], {});
