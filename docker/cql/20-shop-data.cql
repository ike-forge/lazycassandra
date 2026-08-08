-- Every identifier and instant below is a literal. Nothing here calls now(),
-- uuid() or now-derived helpers: re-seeding must produce identical data.

USE shop;

INSERT INTO customers_by_id (customer_id, email, display_name, country_code, signup_date, lifetime_spend, is_active, tags, last_seen_at)
    VALUES (a0000000-0000-4000-8000-000000000001, 'ada.lovelace@example.com', 'Ada Lovelace', 'GB', '2023-04-11', 4821.50, true, {'vip', 'newsletter'}, '2026-02-27T09:14:02.000Z');
INSERT INTO customers_by_id (customer_id, email, display_name, country_code, signup_date, lifetime_spend, is_active, tags, last_seen_at)
    VALUES (a0000000-0000-4000-8000-000000000002, 'grace.hopper@example.com', 'Grace Hopper', 'US', '2022-11-30', 12904.75, true, {'vip', 'wholesale', 'newsletter'}, '2026-03-01T17:45:31.000Z');
INSERT INTO customers_by_id (customer_id, email, display_name, country_code, signup_date, lifetime_spend, is_active, tags, last_seen_at)
    VALUES (a0000000-0000-4000-8000-000000000003, 'alan.turing@example.com', 'Alan Turing', 'GB', '2024-01-07', 318.00, true, {'newsletter'}, '2026-01-19T22:03:55.000Z');
INSERT INTO customers_by_id (customer_id, email, display_name, country_code, signup_date, lifetime_spend, is_active, tags, last_seen_at)
    VALUES (a0000000-0000-4000-8000-000000000004, 'katherine.johnson@example.com', 'Katherine Johnson', 'US', '2021-06-22', 27310.10, false, {}, '2025-08-14T11:27:08.000Z');
INSERT INTO customers_by_id (customer_id, email, display_name, country_code, signup_date, lifetime_spend, is_active, tags, last_seen_at)
    VALUES (a0000000-0000-4000-8000-000000000005, 'barbara.liskov@example.com', 'Barbara Liskov', 'DE', '2025-02-03', 0, true, null, null);
INSERT INTO customers_by_id (customer_id, email, display_name, country_code, signup_date, lifetime_spend, is_active, tags, last_seen_at)
    VALUES (a0000000-0000-4000-8000-000000000006, 'karen.sparck-jones@example.com', 'Karen Spärck Jones', 'GB', '2024-09-18', 1990.99, true, {'wholesale'}, '2026-02-11T06:52:44.000Z');

INSERT INTO orders_by_customer (customer_id, order_date, order_id, status, total_amount, currency, item_count, shipped_at)
    VALUES (a0000000-0000-4000-8000-000000000001, '2026-02-14', f3a54e00-2b00-11f0-8000-0242ac120001, 'DELIVERED', 129.99, 'GBP', 3, '2026-02-16T08:00:00.000Z');
INSERT INTO orders_by_customer (customer_id, order_date, order_id, status, total_amount, currency, item_count, shipped_at)
    VALUES (a0000000-0000-4000-8000-000000000001, '2026-01-03', f3a54e00-2b00-11f0-8000-0242ac120002, 'DELIVERED', 42.00, 'GBP', 1, '2026-01-05T14:30:00.000Z');
INSERT INTO orders_by_customer (customer_id, order_date, order_id, status, total_amount, currency, item_count, shipped_at)
    VALUES (a0000000-0000-4000-8000-000000000001, '2025-12-24', f3a54e00-2b00-11f0-8000-0242ac120003, 'REFUNDED', 310.45, 'GBP', 5, '2025-12-27T09:15:00.000Z');
INSERT INTO orders_by_customer (customer_id, order_date, order_id, status, total_amount, currency, item_count, shipped_at)
    VALUES (a0000000-0000-4000-8000-000000000002, '2026-03-01', f3a54e00-2b00-11f0-8000-0242ac120004, 'PENDING', 8420.00, 'USD', 112, null);
INSERT INTO orders_by_customer (customer_id, order_date, order_id, status, total_amount, currency, item_count, shipped_at)
    VALUES (a0000000-0000-4000-8000-000000000002, '2026-02-28', f3a54e00-2b00-11f0-8000-0242ac120005, 'SHIPPED', 1204.99, 'USD', 18, '2026-03-01T02:10:00.000Z');
INSERT INTO orders_by_customer (customer_id, order_date, order_id, status, total_amount, currency, item_count, shipped_at)
    VALUES (a0000000-0000-4000-8000-000000000003, '2026-01-19', f3a54e00-2b00-11f0-8000-0242ac120006, 'CANCELLED', 318.00, 'GBP', 2, null);
INSERT INTO orders_by_customer (customer_id, order_date, order_id, status, total_amount, currency, item_count, shipped_at)
    VALUES (a0000000-0000-4000-8000-000000000006, '2026-02-11', f3a54e00-2b00-11f0-8000-0242ac120007, 'DELIVERED', 1990.99, 'EUR', 44, '2026-02-13T16:40:00.000Z');

INSERT INTO order_items_by_order (order_id, line_no, sku, description, quantity, unit_price, discount_pct)
    VALUES (f3a54e00-2b00-11f0-8000-0242ac120001, 1, 'KB-MX-87', 'Mechanical keyboard, 87 key, tactile', 1, 89.99, 0.0);
INSERT INTO order_items_by_order (order_id, line_no, sku, description, quantity, unit_price, discount_pct)
    VALUES (f3a54e00-2b00-11f0-8000-0242ac120001, 2, 'CBL-USBC-2M', 'USB-C cable, braided, 2 m', 2, 12.50, 10.0);
INSERT INTO order_items_by_order (order_id, line_no, sku, description, quantity, unit_price, discount_pct)
    VALUES (f3a54e00-2b00-11f0-8000-0242ac120001, 3, 'MAT-DESK-L', 'Desk mat, large, charcoal', 1, 24.00, 0.0);
INSERT INTO order_items_by_order (order_id, line_no, sku, description, quantity, unit_price, discount_pct)
    VALUES (f3a54e00-2b00-11f0-8000-0242ac120004, 1, 'SRV-RACK-42U', 'Server rack, 42U, vented door', 4, 1850.00, 5.0);
INSERT INTO order_items_by_order (order_id, line_no, sku, description, quantity, unit_price, discount_pct)
    VALUES (f3a54e00-2b00-11f0-8000-0242ac120004, 2, 'PDU-16A-C13', 'Rack PDU, 16 A, 24 x C13', 8, 129.00, 0.0);
INSERT INTO order_items_by_order (order_id, line_no, sku, description, quantity, unit_price, discount_pct)
    VALUES (f3a54e00-2b00-11f0-8000-0242ac120006, 1, 'BOOK-CS-001', 'Computing Machinery and Intelligence, reprint', 2, 159.00, 0.0);

INSERT INTO products_by_category (category, price, product_id, name, brand, in_stock, weight_kg, attributes)
    VALUES ('peripherals', 89.99, b0000000-0000-4000-8000-000000000001, 'Mechanical Keyboard 87', 'Northgate', true, 0.94, {'switch': 'tactile brown', 'layout': 'ISO-UK', 'backlight': 'white'});
INSERT INTO products_by_category (category, price, product_id, name, brand, in_stock, weight_kg, attributes)
    VALUES ('peripherals', 24.00, b0000000-0000-4000-8000-000000000002, 'Desk Mat Large', 'Northgate', true, 0.41, {'material': 'felt', 'colour': 'charcoal'});
INSERT INTO products_by_category (category, price, product_id, name, brand, in_stock, weight_kg, attributes)
    VALUES ('peripherals', 12.50, b0000000-0000-4000-8000-000000000003, 'USB-C Cable 2m', 'Corelink', false, 0.08, {'length_m': '2', 'rating': '100W'});
INSERT INTO products_by_category (category, price, product_id, name, brand, in_stock, weight_kg, attributes)
    VALUES ('datacentre', 1850.00, b0000000-0000-4000-8000-000000000004, 'Server Rack 42U', 'Ironframe', true, 118.00, {'height_u': '42', 'door': 'vented'});
INSERT INTO products_by_category (category, price, product_id, name, brand, in_stock, weight_kg, attributes)
    VALUES ('datacentre', 129.00, b0000000-0000-4000-8000-000000000005, 'Rack PDU 16A', 'Ironframe', true, 4.20, {'outlets': '24', 'plug': 'C20'});
INSERT INTO products_by_category (category, price, product_id, name, brand, in_stock, weight_kg, attributes)
    VALUES ('books', 159.00, b0000000-0000-4000-8000-000000000006, 'Computing Machinery and Intelligence', 'Mind Press', true, 0.32, {});

INSERT INTO inventory_by_warehouse (warehouse_id, sku_prefix, sku, on_hand, reserved, reorder_at, updated_at)
    VALUES ('LHR-01', 'KB', 'KB-MX-87', 412, 18, 100, '2026-03-01T06:00:00.000Z');
INSERT INTO inventory_by_warehouse (warehouse_id, sku_prefix, sku, on_hand, reserved, reorder_at, updated_at)
    VALUES ('LHR-01', 'KB', 'KB-MX-104', 87, 0, 100, '2026-03-01T06:00:00.000Z');
INSERT INTO inventory_by_warehouse (warehouse_id, sku_prefix, sku, on_hand, reserved, reorder_at, updated_at)
    VALUES ('LHR-01', 'CBL', 'CBL-USBC-2M', 0, 0, 500, '2026-02-28T06:00:00.000Z');
INSERT INTO inventory_by_warehouse (warehouse_id, sku_prefix, sku, on_hand, reserved, reorder_at, updated_at)
    VALUES ('FRA-02', 'SRV', 'SRV-RACK-42U', 9, 4, 5, '2026-03-01T06:00:00.000Z');
INSERT INTO inventory_by_warehouse (warehouse_id, sku_prefix, sku, on_hand, reserved, reorder_at, updated_at)
    VALUES ('FRA-02', 'PDU', 'PDU-16A-C13', 231, 8, 50, '2026-03-01T06:00:00.000Z');

-- product_view_counts is seeded by seed.sh, not here: a counter UPDATE is an
-- increment rather than an upsert, so replaying this file would multiply the
-- totals by the number of runs.

-- Neighbouring days of the wide partition, kept deliberately small.
INSERT INTO audit_log_by_day (day, logged_at, event_id, actor, action, target, detail)
    VALUES ('2026-02-28', '2026-02-28T23:59:01.000Z', c0000000-0000-4000-8000-000000000001, 'grace.hopper@example.com', 'ORDER_PLACED', 'order:f3a54e00-2b00-11f0-8000-0242ac120005', 'checkout via web');
INSERT INTO audit_log_by_day (day, logged_at, event_id, actor, action, target, detail)
    VALUES ('2026-03-02', '2026-03-02T00:00:12.000Z', c0000000-0000-4000-8000-000000000002, 'system', 'REPLICATION_REPAIR', 'keyspace:shop', 'scheduled repair completed');
