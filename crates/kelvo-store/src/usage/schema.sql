CREATE TABLE usage_apps (
 id INTEGER PRIMARY KEY, host_id INTEGER NOT NULL REFERENCES hosts(id),
 name TEXT NOT NULL, last_ms INTEGER NOT NULL,
 UNIQUE(host_id,name)
);
CREATE TABLE usage_processes (
 id INTEGER PRIMARY KEY, host_id INTEGER NOT NULL REFERENCES hosts(id),
 app_id INTEGER NOT NULL, pid INTEGER NOT NULL, start_time_us INTEGER NOT NULL,
 name TEXT NOT NULL, last_ms INTEGER NOT NULL,
 UNIQUE(host_id,pid,start_time_us)
);
CREATE INDEX usage_process_app ON usage_processes(host_id,app_id);
CREATE TABLE usage_app_buckets (
 host_id INTEGER NOT NULL REFERENCES hosts(id), width_ms INTEGER NOT NULL,
 bucket_ts INTEGER NOT NULL, seq INTEGER NOT NULL, blob BLOB NOT NULL,
 PRIMARY KEY(host_id,width_ms,bucket_ts)
);
CREATE TABLE usage_process_buckets (
 host_id INTEGER NOT NULL REFERENCES hosts(id), width_ms INTEGER NOT NULL,
 bucket_ts INTEGER NOT NULL, seq INTEGER NOT NULL, blob BLOB NOT NULL,
 PRIMARY KEY(host_id,width_ms,bucket_ts)
);
CREATE TABLE usage_coverage (
 host_id INTEGER NOT NULL REFERENCES hosts(id), width_ms INTEGER NOT NULL,
 bucket_ts INTEGER NOT NULL, seq INTEGER NOT NULL, blob BLOB NOT NULL,
 PRIMARY KEY(host_id,width_ms,bucket_ts)
);
CREATE TABLE usage_uncertain (
 id INTEGER PRIMARY KEY, host_id INTEGER NOT NULL REFERENCES hosts(id),
 app_id INTEGER, possible_from_ms INTEGER NOT NULL, possible_to_ms INTEGER NOT NULL,
 rx_bytes INTEGER NOT NULL, tx_bytes INTEGER NOT NULL, records INTEGER NOT NULL, seq INTEGER NOT NULL,
 UNIQUE(host_id,app_id,possible_from_ms,possible_to_ms,rx_bytes,tx_bytes)
);
CREATE UNIQUE INDEX usage_uncertain_identity ON usage_uncertain(host_id,coalesce(app_id,0),possible_from_ms,possible_to_ms,rx_bytes,tx_bytes);
CREATE INDEX usage_uncertain_range ON usage_uncertain(host_id,possible_to_ms,possible_from_ms);
CREATE TABLE usage_recording_start (
 host_id INTEGER PRIMARY KEY REFERENCES hosts(id), from_ms INTEGER NOT NULL, retained_from_ms INTEGER NOT NULL DEFAULT 0
);
