-- Independent Docker clocks and entity-local layouts. Composite FKs enforce ownership.
CREATE TABLE docker_engines (
 id INTEGER PRIMARY KEY, host_id INTEGER NOT NULL REFERENCES hosts(id), daemon_id TEXT NOT NULL,
 runtime TEXT NOT NULL, last_seen_ms INTEGER NOT NULL, generation INTEGER NOT NULL,
 durable_revision INTEGER NOT NULL DEFAULT 0,
 UNIQUE(host_id, daemon_id), UNIQUE(id, host_id)
);
CREATE TABLE docker_entities (
 id INTEGER PRIMARY KEY, host_id INTEGER NOT NULL, engine_id INTEGER NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('container','aggregate')), external_id TEXT NOT NULL,
 UNIQUE(engine_id, kind, external_id), UNIQUE(id, engine_id, host_id),
 FOREIGN KEY(engine_id, host_id) REFERENCES docker_engines(id, host_id) ON DELETE CASCADE,
 CHECK((kind='aggregate' AND external_id='') OR (kind='container' AND external_id<>''))
);
CREATE TABLE docker_metadata (
 entity_id INTEGER NOT NULL, engine_id INTEGER NOT NULL, host_id INTEGER NOT NULL,
 effective_ms INTEGER NOT NULL, revision INTEGER NOT NULL,
 name TEXT NOT NULL, image TEXT NOT NULL, project TEXT, service TEXT, lifecycle TEXT NOT NULL,
 explicit_limit INTEGER, effective_limit INTEGER, capacity INTEGER, last_seen_ms INTEGER NOT NULL,
 tombstone INTEGER NOT NULL, extras BLOB NOT NULL,
 PRIMARY KEY(entity_id,effective_ms,revision),
 FOREIGN KEY(entity_id,engine_id,host_id) REFERENCES docker_entities(id,engine_id,host_id) ON DELETE CASCADE
);
CREATE TABLE docker_metrics (id INTEGER PRIMARY KEY, metric_key TEXT NOT NULL UNIQUE, unit TEXT NOT NULL, kind TEXT NOT NULL);
CREATE TABLE docker_layouts (id INTEGER PRIMARY KEY, version INTEGER NOT NULL, hash TEXT NOT NULL UNIQUE);
CREATE TABLE docker_layout_members (
 layout_id INTEGER NOT NULL REFERENCES docker_layouts(id) ON DELETE CASCADE,
 ordinal INTEGER NOT NULL, metric_id INTEGER NOT NULL REFERENCES docker_metrics(id),
 PRIMARY KEY(layout_id,ordinal), UNIQUE(layout_id,metric_id)
);
CREATE TABLE docker_populations (
 id INTEGER PRIMARY KEY, host_id INTEGER NOT NULL, engine_id INTEGER NOT NULL,
 revision INTEGER NOT NULL, effective_ms INTEGER NOT NULL,
 UNIQUE(engine_id,revision), UNIQUE(id,engine_id,host_id),
 FOREIGN KEY(engine_id,host_id) REFERENCES docker_engines(id,host_id) ON DELETE CASCADE
);
CREATE TABLE docker_population_members (
 population_id INTEGER NOT NULL, entity_id INTEGER NOT NULL, engine_id INTEGER NOT NULL, host_id INTEGER NOT NULL,
 PRIMARY KEY(population_id,entity_id),
 FOREIGN KEY(population_id,engine_id,host_id) REFERENCES docker_populations(id,engine_id,host_id) ON DELETE CASCADE,
 FOREIGN KEY(entity_id,engine_id,host_id) REFERENCES docker_entities(id,engine_id,host_id) ON DELETE CASCADE
);
CREATE TABLE docker_10s (
 entity_id INTEGER NOT NULL, bucket_ms INTEGER NOT NULL, layout_id INTEGER NOT NULL REFERENCES docker_layouts(id),
 engine_id INTEGER NOT NULL, host_id INTEGER NOT NULL, revision INTEGER NOT NULL, payload BLOB NOT NULL,
 PRIMARY KEY(entity_id,bucket_ms,layout_id), UNIQUE(entity_id,bucket_ms,layout_id,engine_id,host_id),
 FOREIGN KEY(entity_id,engine_id,host_id) REFERENCES docker_entities(id,engine_id,host_id) ON DELETE CASCADE
);
CREATE TABLE docker_1m (
 entity_id INTEGER NOT NULL, bucket_ms INTEGER NOT NULL, layout_id INTEGER NOT NULL REFERENCES docker_layouts(id),
 engine_id INTEGER NOT NULL, host_id INTEGER NOT NULL, revision INTEGER NOT NULL, payload BLOB NOT NULL,
 PRIMARY KEY(entity_id,bucket_ms,layout_id), UNIQUE(entity_id,bucket_ms,layout_id,engine_id,host_id),
 FOREIGN KEY(entity_id,engine_id,host_id) REFERENCES docker_entities(id,engine_id,host_id) ON DELETE CASCADE
);
CREATE TABLE docker_15m (
 entity_id INTEGER NOT NULL, bucket_ms INTEGER NOT NULL, layout_id INTEGER NOT NULL REFERENCES docker_layouts(id),
 engine_id INTEGER NOT NULL, host_id INTEGER NOT NULL, revision INTEGER NOT NULL, payload BLOB NOT NULL,
 PRIMARY KEY(entity_id,bucket_ms,layout_id), UNIQUE(entity_id,bucket_ms,layout_id,engine_id,host_id),
 FOREIGN KEY(entity_id,engine_id,host_id) REFERENCES docker_entities(id,engine_id,host_id) ON DELETE CASCADE
);
CREATE INDEX docker_10s_engine ON docker_10s(engine_id,bucket_ms);
CREATE INDEX docker_1m_engine ON docker_1m(engine_id,bucket_ms);
CREATE INDEX docker_15m_engine ON docker_15m(engine_id,bucket_ms);
CREATE TABLE docker_10s_populations (
 entity_id INTEGER NOT NULL, bucket_ms INTEGER NOT NULL, layout_id INTEGER NOT NULL,
 engine_id INTEGER NOT NULL, host_id INTEGER NOT NULL, segment INTEGER NOT NULL,
 start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL CHECK(end_ms>=start_ms), population_id INTEGER NOT NULL,
 PRIMARY KEY(entity_id,bucket_ms,layout_id,segment),
 FOREIGN KEY(entity_id,bucket_ms,layout_id,engine_id,host_id) REFERENCES docker_10s(entity_id,bucket_ms,layout_id,engine_id,host_id) ON DELETE CASCADE,
 FOREIGN KEY(population_id,engine_id,host_id) REFERENCES docker_populations(id,engine_id,host_id)
);
CREATE TABLE docker_1m_populations (
 entity_id INTEGER NOT NULL, bucket_ms INTEGER NOT NULL, layout_id INTEGER NOT NULL,
 engine_id INTEGER NOT NULL, host_id INTEGER NOT NULL, segment INTEGER NOT NULL,
 start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL CHECK(end_ms>=start_ms), population_id INTEGER NOT NULL,
 PRIMARY KEY(entity_id,bucket_ms,layout_id,segment),
 FOREIGN KEY(entity_id,bucket_ms,layout_id,engine_id,host_id) REFERENCES docker_1m(entity_id,bucket_ms,layout_id,engine_id,host_id) ON DELETE CASCADE,
 FOREIGN KEY(population_id,engine_id,host_id) REFERENCES docker_populations(id,engine_id,host_id)
);
CREATE TABLE docker_15m_populations (
 entity_id INTEGER NOT NULL, bucket_ms INTEGER NOT NULL, layout_id INTEGER NOT NULL,
 engine_id INTEGER NOT NULL, host_id INTEGER NOT NULL, segment INTEGER NOT NULL,
 start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL CHECK(end_ms>=start_ms), population_id INTEGER NOT NULL,
 PRIMARY KEY(entity_id,bucket_ms,layout_id,segment),
 FOREIGN KEY(entity_id,bucket_ms,layout_id,engine_id,host_id) REFERENCES docker_15m(entity_id,bucket_ms,layout_id,engine_id,host_id) ON DELETE CASCADE,
 FOREIGN KEY(population_id,engine_id,host_id) REFERENCES docker_populations(id,engine_id,host_id)
);
CREATE TABLE docker_events (
 id INTEGER PRIMARY KEY, host_id INTEGER NOT NULL, engine_id INTEGER NOT NULL, entity_id INTEGER,
 event_key TEXT NOT NULL, time_ns INTEGER NOT NULL, time_ms INTEGER NOT NULL,
 action TEXT NOT NULL, payload BLOB NOT NULL, UNIQUE(engine_id,event_key),
 FOREIGN KEY(engine_id,host_id) REFERENCES docker_engines(id,host_id) ON DELETE CASCADE,
 FOREIGN KEY(entity_id,engine_id,host_id) REFERENCES docker_entities(id,engine_id,host_id) ON DELETE CASCADE
);
CREATE INDEX docker_events_engine_time ON docker_events(engine_id,time_ms,id);
CREATE INDEX docker_events_entity_time ON docker_events(entity_id,time_ms,id);
CREATE INDEX docker_events_engine_identity ON docker_events(engine_id,time_ns,event_key);
CREATE INDEX docker_events_entity_identity ON docker_events(entity_id,time_ns,event_key);
CREATE TABLE docker_coverage (
 id INTEGER PRIMARY KEY, host_id INTEGER NOT NULL, engine_id INTEGER NOT NULL, entity_id INTEGER,
 metric TEXT, start_ms INTEGER NOT NULL, end_ms INTEGER, reason TEXT NOT NULL, revision INTEGER NOT NULL,
 sampled_ms INTEGER NOT NULL, expected_ms INTEGER NOT NULL,
 CHECK(end_ms IS NULL OR end_ms>=start_ms),
 FOREIGN KEY(engine_id,host_id) REFERENCES docker_engines(id,host_id) ON DELETE CASCADE,
 FOREIGN KEY(entity_id,engine_id,host_id) REFERENCES docker_entities(id,engine_id,host_id) ON DELETE CASCADE
);
CREATE INDEX docker_coverage_engine_range ON docker_coverage(engine_id,start_ms);
CREATE INDEX docker_coverage_entity_range ON docker_coverage(entity_id,start_ms);
CREATE TABLE docker_event_cursors (
 engine_id INTEGER PRIMARY KEY REFERENCES docker_engines(id) ON DELETE CASCADE,
 time_ns INTEGER NOT NULL, event_key TEXT NOT NULL, generation INTEGER NOT NULL
);
CREATE TABLE docker_accounting (
 engine_id INTEGER NOT NULL REFERENCES docker_engines(id) ON DELETE CASCADE,
 class TEXT NOT NULL, logical_bytes INTEGER NOT NULL CHECK(logical_bytes>=0), PRIMARY KEY(engine_id,class)
);

CREATE TRIGGER docker_entities_account_insert AFTER INSERT ON docker_entities BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_entities',max(0,2*(128+length(NEW.external_id))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(128+length(NEW.external_id))));
END;

CREATE TRIGGER docker_entities_account_delete AFTER DELETE ON docker_entities BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_entities',max(0,-(2*(128+length(OLD.external_id)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(2*(128+length(OLD.external_id)))));
END;

CREATE TRIGGER docker_entities_account_update AFTER UPDATE ON docker_entities BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_entities',max(0,2*(128+length(NEW.external_id))-(2*(128+length(OLD.external_id)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(128+length(NEW.external_id))-(2*(128+length(OLD.external_id)))));
END;

CREATE TRIGGER docker_metadata_account_insert AFTER INSERT ON docker_metadata BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_metadata',max(0,2*(128+length(NEW.name)+length(NEW.image)+coalesce(length(NEW.project),0)+coalesce(length(NEW.service),0)+length(NEW.lifecycle)+length(NEW.extras))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(128+length(NEW.name)+length(NEW.image)+coalesce(length(NEW.project),0)+coalesce(length(NEW.service),0)+length(NEW.lifecycle)+length(NEW.extras))));
END;

CREATE TRIGGER docker_metadata_account_delete AFTER DELETE ON docker_metadata BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_metadata',max(0,-(2*(128+length(OLD.name)+length(OLD.image)+coalesce(length(OLD.project),0)+coalesce(length(OLD.service),0)+length(OLD.lifecycle)+length(OLD.extras)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(2*(128+length(OLD.name)+length(OLD.image)+coalesce(length(OLD.project),0)+coalesce(length(OLD.service),0)+length(OLD.lifecycle)+length(OLD.extras)))));
END;

CREATE TRIGGER docker_metadata_account_update AFTER UPDATE ON docker_metadata BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_metadata',max(0,2*(128+length(NEW.name)+length(NEW.image)+coalesce(length(NEW.project),0)+coalesce(length(NEW.service),0)+length(NEW.lifecycle)+length(NEW.extras))-(2*(128+length(OLD.name)+length(OLD.image)+coalesce(length(OLD.project),0)+coalesce(length(OLD.service),0)+length(OLD.lifecycle)+length(OLD.extras)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(128+length(NEW.name)+length(NEW.image)+coalesce(length(NEW.project),0)+coalesce(length(NEW.service),0)+length(NEW.lifecycle)+length(NEW.extras))-(2*(128+length(OLD.name)+length(OLD.image)+coalesce(length(OLD.project),0)+coalesce(length(OLD.service),0)+length(OLD.lifecycle)+length(OLD.extras)))));
END;

CREATE TRIGGER docker_populations_account_insert AFTER INSERT ON docker_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_populations',max(0,256))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(256));
END;

CREATE TRIGGER docker_populations_account_delete AFTER DELETE ON docker_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_populations',max(0,-(256)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(256)));
END;

CREATE TRIGGER docker_populations_account_update AFTER UPDATE ON docker_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_populations',max(0,256-(256)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(256-(256)));
END;

CREATE TRIGGER docker_population_members_account_insert AFTER INSERT ON docker_population_members BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_population_members',max(0,192))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(192));
END;

CREATE TRIGGER docker_population_members_account_delete AFTER DELETE ON docker_population_members BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_population_members',max(0,-(192)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(192)));
END;

CREATE TRIGGER docker_population_members_account_update AFTER UPDATE ON docker_population_members BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_population_members',max(0,192-(192)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(192-(192)));
END;

CREATE TRIGGER docker_10s_account_insert AFTER INSERT ON docker_10s BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_10s',max(0,512+2*length(NEW.payload)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(512+2*length(NEW.payload)));
END;

CREATE TRIGGER docker_10s_account_delete AFTER DELETE ON docker_10s BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_10s',max(0,-(512+2*length(OLD.payload))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(512+2*length(OLD.payload))));
END;

CREATE TRIGGER docker_10s_account_update AFTER UPDATE ON docker_10s BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_10s',max(0,512+2*length(NEW.payload)-(512+2*length(OLD.payload))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(512+2*length(NEW.payload)-(512+2*length(OLD.payload))));
END;

CREATE TRIGGER docker_1m_account_insert AFTER INSERT ON docker_1m BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_1m',max(0,512+2*length(NEW.payload)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(512+2*length(NEW.payload)));
END;

CREATE TRIGGER docker_1m_account_delete AFTER DELETE ON docker_1m BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_1m',max(0,-(512+2*length(OLD.payload))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(512+2*length(OLD.payload))));
END;

CREATE TRIGGER docker_1m_account_update AFTER UPDATE ON docker_1m BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_1m',max(0,512+2*length(NEW.payload)-(512+2*length(OLD.payload))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(512+2*length(NEW.payload)-(512+2*length(OLD.payload))));
END;

CREATE TRIGGER docker_15m_account_insert AFTER INSERT ON docker_15m BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_15m',max(0,512+2*length(NEW.payload)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(512+2*length(NEW.payload)));
END;

CREATE TRIGGER docker_15m_account_delete AFTER DELETE ON docker_15m BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_15m',max(0,-(512+2*length(OLD.payload))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(512+2*length(OLD.payload))));
END;

CREATE TRIGGER docker_15m_account_update AFTER UPDATE ON docker_15m BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_15m',max(0,512+2*length(NEW.payload)-(512+2*length(OLD.payload))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(512+2*length(NEW.payload)-(512+2*length(OLD.payload))));
END;

CREATE TRIGGER docker_10s_populations_account_insert AFTER INSERT ON docker_10s_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_10s_populations',max(0,256))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(256));
END;

CREATE TRIGGER docker_10s_populations_account_delete AFTER DELETE ON docker_10s_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_10s_populations',max(0,-(256)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(256)));
END;

CREATE TRIGGER docker_10s_populations_account_update AFTER UPDATE ON docker_10s_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_10s_populations',max(0,256-(256)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(256-(256)));
END;

CREATE TRIGGER docker_1m_populations_account_insert AFTER INSERT ON docker_1m_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_1m_populations',max(0,256))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(256));
END;

CREATE TRIGGER docker_1m_populations_account_delete AFTER DELETE ON docker_1m_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_1m_populations',max(0,-(256)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(256)));
END;

CREATE TRIGGER docker_1m_populations_account_update AFTER UPDATE ON docker_1m_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_1m_populations',max(0,256-(256)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(256-(256)));
END;

CREATE TRIGGER docker_15m_populations_account_insert AFTER INSERT ON docker_15m_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_15m_populations',max(0,256))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(256));
END;

CREATE TRIGGER docker_15m_populations_account_delete AFTER DELETE ON docker_15m_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_15m_populations',max(0,-(256)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(256)));
END;

CREATE TRIGGER docker_15m_populations_account_update AFTER UPDATE ON docker_15m_populations BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_15m_populations',max(0,256-(256)))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(256-(256)));
END;

CREATE TRIGGER docker_events_account_insert AFTER INSERT ON docker_events BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_events',max(0,2*(128+length(NEW.event_key)+length(NEW.action)+length(NEW.payload))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(128+length(NEW.event_key)+length(NEW.action)+length(NEW.payload))));
END;

CREATE TRIGGER docker_events_account_delete AFTER DELETE ON docker_events BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_events',max(0,-(2*(128+length(OLD.event_key)+length(OLD.action)+length(OLD.payload)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(2*(128+length(OLD.event_key)+length(OLD.action)+length(OLD.payload)))));
END;

CREATE TRIGGER docker_events_account_update AFTER UPDATE ON docker_events BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_events',max(0,2*(128+length(NEW.event_key)+length(NEW.action)+length(NEW.payload))-(2*(128+length(OLD.event_key)+length(OLD.action)+length(OLD.payload)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(128+length(NEW.event_key)+length(NEW.action)+length(NEW.payload))-(2*(128+length(OLD.event_key)+length(OLD.action)+length(OLD.payload)))));
END;

CREATE TRIGGER docker_coverage_account_insert AFTER INSERT ON docker_coverage BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_coverage',max(0,2*(128+length(NEW.reason)+coalesce(length(NEW.metric),0))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(128+length(NEW.reason)+coalesce(length(NEW.metric),0))));
END;

CREATE TRIGGER docker_coverage_account_delete AFTER DELETE ON docker_coverage BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_coverage',max(0,-(2*(128+length(OLD.reason)+coalesce(length(OLD.metric),0)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(2*(128+length(OLD.reason)+coalesce(length(OLD.metric),0)))));
END;

CREATE TRIGGER docker_coverage_account_update AFTER UPDATE ON docker_coverage BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_coverage',max(0,2*(128+length(NEW.reason)+coalesce(length(NEW.metric),0))-(2*(128+length(OLD.reason)+coalesce(length(OLD.metric),0)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(128+length(NEW.reason)+coalesce(length(NEW.metric),0))-(2*(128+length(OLD.reason)+coalesce(length(OLD.metric),0)))));
END;

CREATE TRIGGER docker_event_cursors_account_insert AFTER INSERT ON docker_event_cursors BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_event_cursors',max(0,2*(64+length(NEW.event_key))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(64+length(NEW.event_key))));
END;

CREATE TRIGGER docker_event_cursors_account_delete AFTER DELETE ON docker_event_cursors BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(OLD.engine_id,'docker_event_cursors',max(0,-(2*(64+length(OLD.event_key)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(-(2*(64+length(OLD.event_key)))));
END;

CREATE TRIGGER docker_event_cursors_account_update AFTER UPDATE ON docker_event_cursors BEGIN
 INSERT INTO docker_accounting(engine_id,class,logical_bytes) VALUES(NEW.engine_id,'docker_event_cursors',max(0,2*(64+length(NEW.event_key))-(2*(64+length(OLD.event_key)))))
 ON CONFLICT(engine_id,class) DO UPDATE SET logical_bytes=max(0,logical_bytes+(2*(64+length(NEW.event_key))-(2*(64+length(OLD.event_key)))));
END;

-- Global oldest-row reclamation uses one indexed minimum per table, then a bounded
-- metadata predecessor pass; it must not sort the entire history for each deleted row.
CREATE INDEX docker_10s_oldest ON docker_10s(bucket_ms);
CREATE INDEX docker_1m_oldest ON docker_1m(bucket_ms);
CREATE INDEX docker_15m_oldest ON docker_15m(bucket_ms);
CREATE INDEX docker_events_oldest ON docker_events(time_ms);
CREATE INDEX docker_coverage_oldest ON docker_coverage(end_ms);
