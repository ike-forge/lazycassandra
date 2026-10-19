-- A query that takes as long as it is told to, for exercising cancellation.
--
--   SELECT edge_cases.busy_wait(k, 30000) FROM edge_cases.t LIMIT 1;
--
-- Spins on the node's clock: Cassandra has no sleep, and a scan big enough to
-- be slow is not reproducible. Needs the UDF settings in compose.yaml.

USE edge_cases;

CREATE OR REPLACE FUNCTION busy_wait(k int, millis int)
    CALLED ON NULL INPUT RETURNS int LANGUAGE java AS $$
        long end = System.nanoTime() + millis * 1000000L;
        while (System.nanoTime() < end) { }
        return k;
    $$;
