-- Three keyspaces, deliberately differing in replication strategy and
-- durability so the keyspace list has something to render besides names.
--
-- Every RF is 1: the fixture is a single node, and NetworkTopologyStrategy
-- with an unmet RF would silently under-replicate on write.

CREATE KEYSPACE IF NOT EXISTS shop
    WITH replication = {'class': 'NetworkTopologyStrategy', 'datacenter1': 1}
    AND durable_writes = true;

CREATE KEYSPACE IF NOT EXISTS typezoo
    WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}
    AND durable_writes = true;

CREATE KEYSPACE IF NOT EXISTS edge_cases
    WITH replication = {'class': 'NetworkTopologyStrategy', 'datacenter1': 1}
    AND durable_writes = false;
