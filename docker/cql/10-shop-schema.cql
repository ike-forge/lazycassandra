-- The nominal keyspace. Ordinary identifier lengths, realistic column names,
-- query-driven denormalisation. This is the schema the TUI should feel good
-- on; the other two keyspaces are there to stop it from breaking.

USE shop;

CREATE TABLE IF NOT EXISTS customers_by_id (
    customer_id    uuid,
    email          text,
    display_name   text,
    country_code   text,
    signup_date    date,
    lifetime_spend decimal,
    is_active      boolean,
    tags           set<text>,
    last_seen_at   timestamp,
    PRIMARY KEY ((customer_id))
) WITH comment = 'Customer master record, keyed by surrogate id.';

CREATE INDEX IF NOT EXISTS customers_by_id_country_code_idx
    ON customers_by_id (country_code);

-- A collection index, so the Indexes tab has a `values(...)` target to read
-- and not only a bare column.
CREATE INDEX IF NOT EXISTS customers_by_id_tags_idx
    ON customers_by_id (tags);

CREATE MATERIALIZED VIEW IF NOT EXISTS customers_by_email AS
    SELECT customer_id, email, display_name, country_code, is_active, signup_date
    FROM customers_by_id
    WHERE email IS NOT NULL AND customer_id IS NOT NULL
    PRIMARY KEY ((email), customer_id);

CREATE TABLE IF NOT EXISTS orders_by_customer (
    customer_id  uuid,
    order_date   date,
    order_id     timeuuid,
    status       text,
    total_amount decimal,
    currency     text,
    item_count   int,
    shipped_at   timestamp,
    PRIMARY KEY ((customer_id), order_date, order_id)
) WITH CLUSTERING ORDER BY (order_date DESC, order_id ASC)
  AND comment = 'Order history for one customer, newest day first.';

CREATE TABLE IF NOT EXISTS order_items_by_order (
    order_id     timeuuid,
    line_no      int,
    sku          text,
    description  text,
    quantity     int,
    unit_price   decimal,
    discount_pct float,
    PRIMARY KEY ((order_id), line_no)
);

CREATE TABLE IF NOT EXISTS products_by_category (
    category   text,
    price      decimal,
    product_id uuid,
    name       text,
    brand      text,
    in_stock   boolean,
    weight_kg  double,
    attributes map<text, text>,
    PRIMARY KEY ((category), price, product_id)
) WITH CLUSTERING ORDER BY (price ASC, product_id ASC);

CREATE TABLE IF NOT EXISTS inventory_by_warehouse (
    warehouse_id text,
    sku_prefix   text,
    sku          text,
    on_hand      int,
    reserved     int,
    reorder_at   int,
    updated_at   timestamp,
    PRIMARY KEY ((warehouse_id, sku_prefix), sku)
) WITH comment = 'Composite partition key: (warehouse_id, sku_prefix).';

CREATE TABLE IF NOT EXISTS product_view_counts (
    product_id  uuid,
    day         date,
    views       counter,
    add_to_cart counter,
    PRIMARY KEY ((product_id), day)
);

-- The paging fixture. seed.sh fills 2026-03-01 with 10 000 clustering rows;
-- the neighbouring days stay small so a partition-size contrast is visible.
CREATE TABLE IF NOT EXISTS audit_log_by_day (
    day       date,
    logged_at timestamp,
    event_id  uuid,
    actor     text,
    action    text,
    target    text,
    detail    text,
    PRIMARY KEY ((day), logged_at, event_id)
) WITH CLUSTERING ORDER BY (logged_at ASC, event_id ASC)
  AND comment = 'Wide-partition fixture: 2026-03-01 holds 10 000 rows.';
