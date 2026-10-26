#!/usr/bin/env python3
"""Blocking durable-usage feasibility probe; temporary SQLite files, no app DB writes.

Measures a candidate packed sufficient-statistics layout, not production integration.
The uncompressed layout follows the store's LE blobs; zlib is a conventional lossless
comparison using the Python standard library, not a proposed production dependency.
All candidate tables are counted, including identities and uncertain-time bytes.
"""

import argparse
import json
import random
import sqlite3
import struct
import tempfile
import time
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
# Interned id, CPU seconds, GPU seconds, joules, RAM max, RAM byte-seconds,
# app-present seconds, read/write bytes, rx/tx bytes. Coverage is per bucket.
ROW = struct.Struct("<IdddQddQQ5I5I")
HEADER = struct.Struct("<BII")
COVERAGE = struct.Struct("<qqIIIIII")
IDENTITY = struct.Struct("<iiq")
UNCERTAIN = struct.Struct("<IqqQQ")


def schema(db):
    db.executescript("""
        PRAGMA page_size=16384;
        PRAGMA journal_mode=WAL;
        PRAGMA wal_autocheckpoint=0;
        CREATE TABLE usage_apps(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
        CREATE TABLE usage_processes(id INTEGER PRIMARY KEY, app_id INTEGER NOT NULL,
            pid INTEGER NOT NULL, start_us INTEGER NOT NULL, name TEXT NOT NULL,
            UNIQUE(pid,start_us));
        CREATE TABLE usage_10s(t INTEGER PRIMARY KEY, payload BLOB NOT NULL);
        CREATE TABLE usage_1m(t INTEGER PRIMARY KEY, payload BLOB NOT NULL);
        CREATE TABLE usage_15m(t INTEGER PRIMARY KEY, payload BLOB NOT NULL);
        CREATE TABLE usage_coverage(t INTEGER PRIMARY KEY, payload BLOB NOT NULL);
        CREATE TABLE usage_uncertain(id INTEGER PRIMARY KEY, payload BLOB NOT NULL);
    """)


def values(rng, identity, bucket, app=False, busy=False):
    # Each30s source interval is retained once in three10s slots. Mostly idle
    # workload preserves every process, including its nonzero RAM. Busy workload
    # exercises independently changing counters without truncation.
    active = busy or identity % 10 == 0
    cpu = rng.random()*10 if active else 0.0
    gpu = rng.random() if identity % 100 == 0 else 0.0
    ram = (8 + identity % 200)*1024*1024 + rng.randrange(0,16)*4096
    return (identity, cpu, gpu, cpu*0.03, ram, ram*10.0, 10.0,
            rng.randrange(0,500000) if app and active else 0,
            rng.randrange(0,500000) if app and active else 0,
            *[10000 if (identity+bucket)%97 else 0 for _ in range(5)],
            *[0 if (identity+bucket)%97 else 1 for _ in range(5)])



def accounting(db):
    result = {}
    for table, sql in {
        "usage_apps": "length(name)+8",
        "usage_processes": "length(name)+28",
        "usage_10s": "length(payload)+8",
        "usage_1m": "length(payload)+8",
        "usage_15m": "length(payload)+8",
        "usage_coverage": "length(payload)+8",
        "usage_uncertain": "length(payload)+8",
    }.items():
        result[table] = db.execute(
            f"SELECT count(*),coalesce(sum({sql}),0) FROM {table}"
        ).fetchone()
    return result


def run(processes, apps, churn, compressed, minutes, batch=False, busy=False, columnar=False):
    rng = random.Random(41023)
    with tempfile.TemporaryDirectory(prefix="kelvo-usage-feasibility-") as folder:
        path = Path(folder)/"usage.sqlite"
        db = sqlite3.connect(path)
        schema(db)
        db.executemany("INSERT INTO usage_apps VALUES(?,?)",
                       [(i+1, f"Application {i+1}") for i in range(apps)])
        db.commit()
        wal = path.with_name(path.name+"-wal")
        wal0 = wal.stat().st_size
        pid = 0
        alive = []
        app_by_id = {}
        pending = []
        coverage_pending = []
        for b in range(minutes*6):
            if b == 0 or (churn and b%3 == 0):
                replacement = processes if b == 0 else max(1,processes//20)
                alive = alive[replacement:]
                for i in range(replacement):
                    pid += 1
                    app = i%apps+1
                    db.execute("INSERT INTO usage_processes VALUES(?,?,?,?,?)",
                               (pid, app, pid%99999+100, b*10_000_000+pid,
                                f"Worker {i%20}"))
                    alive.append(pid)
                    app_by_id[pid] = app
            if b%3 == 0:
                proc_rows = [values(rng, i, b, busy=busy) for i in alive]
            app_rows = []
            for app in range(1, apps+1):
                children = [p for p in proc_rows if app_by_id[p[0]] == app]
                # In this fixture every bucket represents one simultaneous batch.
                app_rows.append((app, sum(p[1] for p in children),
                    sum(p[2] for p in children),sum(p[3] for p in children),
                    sum(p[4] for p in children),sum(p[5] for p in children),10.0,
                    rng.randrange(0,500000) if app%10 == 0 else 0,
                    rng.randrange(0,500000) if app%10 == 0 else 0,
                    *[10000 if children else 0 for _ in range(5)],
                    *[int(any(p[14+j] != 0 for p in children)) for j in range(5)]))
            payload = HEADER.pack(1,len(app_rows),len(proc_rows)) + b"".join(
                ROW.pack(*r) for r in app_rows+proc_rows)
            if batch:
                pending.append(payload)
                if (b+1)%30 == 0:
                    if columnar:
                        # Standard column-major packing: field bytes across all rows
                        # and frames; invertible without rounding or dropped values.
                        headers = b"".join(p[:HEADER.size] for p in pending)
                        body = b"".join(p[HEADER.size:] for p in pending)
                        payload = headers+b"".join(body[i::ROW.size] for i in range(ROW.size))
                    else:
                        payload = b"".join(pending)
                    if compressed:
                        payload = zlib.compress(payload)
                    db.execute("INSERT INTO usage_10s VALUES(?,?)", ((b-29)*10000,payload))
                    pending = []
            else:
                if compressed:
                    payload = zlib.compress(payload)
                db.execute("INSERT INTO usage_10s VALUES(?,?)", (b*10000,payload))
            # Separate coverage includes host/process/GPU/energy/network durations
            # and a cause bitfield; no inference from sampling duration alone.
            coverage = COVERAGE.pack(b*10000,(b+1)*10000,10000,10000,10000,10000,10000,0)
            if batch:
                coverage_pending.append(coverage)
                if (b+1)%30 == 0:
                    db.execute("INSERT INTO usage_coverage VALUES(?,?)",
                        ((b-29)*10000,zlib.compress(b"".join(coverage_pending)) if compressed else b"".join(coverage_pending)))
                    coverage_pending = []
            else:
                db.execute("INSERT INTO usage_coverage VALUES(?,?)",(b*10000,coverage))
            if b%60 == 0:
                db.execute("INSERT INTO usage_uncertain(payload) VALUES(?)",
                           (UNCERTAIN.pack(1,max(0,b*10000-600000),b*10000,10000,2000),))
            if (b+1)%30 == 0:
                db.commit()
        db.commit()
        tables = accounting(db)
        scale = 60/minutes
        rows = sum(v[0] for v in tables.values())*scale
        payload = sum(v[1] for v in tables.values())*scale
        wal_bytes = (wal.stat().st_size-wal0)*scale
        # Verify committed facts and identities survive a new connection.
        expected = (minutes*6//30 if batch else minutes*6, pid)
        db.close()
        recovered = sqlite3.connect(path)
        actual = (recovered.execute("SELECT count(*) FROM usage_10s").fetchone()[0],
                  recovered.execute("SELECT count(*) FROM usage_processes").fetchone()[0])
        assert actual == expected, (actual,expected)
        first = recovered.execute("SELECT payload FROM usage_10s ORDER BY t LIMIT 1").fetchone()[0]
        if compressed:
            first = zlib.decompress(first)
        if columnar and batch:
            header_bytes = first[:HEADER.size*30]
            body = first[HEADER.size*30:]
            n = len(body)//ROW.size
            unshuffled = bytearray(len(body))
            for i in range(ROW.size):
                unshuffled[i::ROW.size] = body[i*n:(i+1)*n]
            per_frame = ROW.size*(apps+processes)
            first = b"".join(header_bytes[i*HEADER.size:(i+1)*HEADER.size]+
                unshuffled[i*per_frame:(i+1)*per_frame] for i in range(30))
        version, app_count, proc_count = HEADER.unpack_from(first)
        assert (version,app_count,proc_count) == (1,apps,processes)
        assert len(first) == (HEADER.size+ROW.size*(apps+processes))*(30 if batch else 1)
        recovered.close()
        return dict(processes=processes,apps=apps,churn=churn,compressed=compressed,
                    minutes=minutes,batch=batch,busy=busy,columnar=columnar,rows_per_hour=rows,payload_per_hour=payload,
                    wal_per_hour=wal_bytes,process_identities_recovered=pid,
                    tables={k:dict(rows=v[0]*scale,bytes=v[1]*scale) for k,v in tables.items()})


def merge_rows(target, incoming):
    for row in incoming:
        key=row[0]
        if key not in target:
            target[key]=list(row)
            continue
        dst=target[key]
        for i in (1,2,3,5,6,7,8,9,10,11,12,13):
            dst[i]+=row[i]
        dst[4]=max(dst[4],row[4])
        for i in (14,15,16,17,18):
            dst[i] |= row[i]


def rolled_hour_cost(processes, apps, width_s, keep_rows=False):
    """Merge actual synthetic30s samples; no extrapolation of10s compression."""
    rng=random.Random(41023)
    alive=list(range(1,processes+1))
    app_of={i:(i-1)%apps+1 for i in alive}
    next_id=processes+1
    total=0
    entries=0
    retained=[]
    for start in range(0,3600,width_s):
        app_sums={}
        proc_sums={}
        for at in range(start,start+width_s,min(30,width_s)):
            if at and at%30 == 0:
                replaced=max(1,processes//20)
                alive=alive[replaced:]
                for i in range(replaced):
                    app_of[next_id]=i%apps+1
                    alive.append(next_id)
                    next_id+=1
            source=[list(values(rng,i,at//10)) for i in alive]
            # The facts are interval integrals for this actual30s source span.
            for row in source:
                for i in (1,2,3,5,6,7,8,9,10,11,12,13):
                    row[i]*=min(30,width_s)//10
            merge_rows(proc_sums,source)
            current={}
            for row in source:
                app=app_of[row[0]]
                if app not in current:
                    current[app]=[app,*row[1:]]
                else:
                    dst=current[app]
                    for i in (1,2,3,4,5):
                        dst[i]+=row[i]
                    for i in (14,15,16,17,18):
                        dst[i] |= row[i]
            # App RAM max is concurrent across this source sample; merge_rows
            # subsequently takes max across samples. It never sums process peaks.
            merge_rows(app_sums,current.values())
        rows=list(app_sums.values())+list(proc_sums.values())
        body=b"".join(ROW.pack(*row) for row in rows)
        columnar=b"".join(body[i::ROW.size] for i in range(ROW.size))
        total+=len(zlib.compress(columnar))+HEADER.size+8
        entries+=len(rows)
        if keep_rows:
            retained.append(rows)
    return dict(bucket_seconds=width_s,retained_payload_bytes_per_hour=total,
                retained_fact_entries=entries, retained_rows=retained)


def retained_identity_cost(processes, apps, days, full=False):
    """Actual SQLite cost of full-horizon identities alone, before usage tier data."""
    with tempfile.TemporaryDirectory(prefix="kelvo-usage-retained-") as folder:
        path = Path(folder)/"usage.sqlite"
        db = sqlite3.connect(path)
        schema(db)
        db.executemany("INSERT INTO usage_apps VALUES(?,?)",
            [(i+1,f"Application {i+1}") for i in range(apps)])
        db.commit()
        count = processes + max(1,processes//20)*(days*24*120-1)
        # Same50/30s instance churn as write probe, stable20 worker names,
        # cyclicPID reuse and distinct starttimes. No names or instances removed.
        for start in range(1,count+1,50000):
            db.executemany("INSERT INTO usage_processes VALUES(?,?,?,?,?)",
                ((i,i%apps+1,i%99999+100,1700000000000000+i*600000,
                  f"Worker {i%20}") for i in range(start,min(start+50000,count+1))))
            db.commit()
        if full:
            templates={width:rolled_hour_cost(processes,apps,width,True)["retained_rows"]
                       for width in (10,60,900)}
            for hour in range(days*24):
                width=10 if hour >= (days-1)*24 else 60 if hour >= (days-7)*24 else 900
                table={10:"usage_10s",60:"usage_1m",900:"usage_15m"}[width]
                offset=hour*max(1,processes//20)*120
                for b,rows in enumerate(templates[width]):
                    encoded=[]
                    for j,row in enumerate(rows):
                        shifted=list(row)
                        shifted[0]=((row[0]-1+offset)%apps)+1 if j < apps else row[0]+offset
                        encoded.append(ROW.pack(*shifted))
                    body=b"".join(encoded)
                    payload=zlib.compress(b"".join(body[i::ROW.size] for i in range(ROW.size)))
                    at=(hour*3600+b*width)*1000
                    db.execute(f"INSERT INTO {table} VALUES(?,?)",(at,payload))
                    coverage=COVERAGE.pack(at,at+width*1000,*[width*1000]*5,1)
                    db.execute("INSERT INTO usage_coverage VALUES(?,?)",(at,coverage))
                db.commit()
                if hour%24 == 23:
                    db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        tables = accounting(db)
        db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        db.close()
        reopened = sqlite3.connect(path)
        assert reopened.execute("SELECT count(*) FROM usage_processes").fetchone()[0] == count
        reopened.close()
        tier_costs=[rolled_hour_cost(processes,apps,w) for w in (60,900)]
        for cost in tier_costs:
            cost.pop("retained_rows")
        print(json.dumps(dict(retained_days=days,processes=processes,apps=apps,
            process_identities=count,identity_payload_bytes=sum(v[1] for v in tables.values()),
            retained_database_bytes=path.stat().st_size,
            usage_tier_payload_excluded=not full,
            tables={k:dict(rows=v[0],bytes=v[1]) for k,v in tables.items()},
            actual_rolled_hour_costs=tier_costs,
            projected_1m_6days_bytes=tier_costs[0]["retained_payload_bytes_per_hour"]*6*24,
            projected_15m_23days_bytes=tier_costs[1]["retained_payload_bytes_per_hour"]*23*24,
            projection_excludes_10s_last_day=True),sort_keys=True),flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--minutes",type=int,default=60)
    parser.add_argument("--processes",type=int,default=400)
    parser.add_argument("--apps",type=int,default=120)
    parser.add_argument("--batch",action="store_true")
    parser.add_argument("--busy",action="store_true")
    parser.add_argument("--columnar",action="store_true")
    parser.add_argument("--retained-days",type=int,default=0)
    parser.add_argument("--full-retained",action="store_true")
    parser.add_argument("--baseline-payload",type=int,required=True)
    parser.add_argument("--baseline-rows",type=int,required=True)
    parser.add_argument("--baseline-wal",type=int,required=True)
    args = parser.parse_args()
    if min(args.minutes,args.processes,args.apps) <= 0 or args.apps > args.processes:
        parser.error("positive minutes/processes/apps required; apps <= processes")
    budgets = json.loads((ROOT/"perf-budget.json").read_text())["engine"]["store"]
    baseline = dict(payload=args.baseline_payload, rows=args.baseline_rows, wal=args.baseline_wal)
    if args.retained_days:
        retained_identity_cost(args.processes,args.apps,args.retained_days,args.full_retained)
        return
    for churn in (False,True):
        for compressed in (False,True):
            result = run(args.processes,args.apps,churn,compressed,args.minutes,args.batch,args.busy,args.columnar)
            result["combined"] = {
                "payload": result["payload_per_hour"]+baseline["payload"],
                "rows": result["rows_per_hour"]+baseline["rows"],
                "wal": result["wal_per_hour"]+baseline["wal"],
            }
            result["within_unchanged_budgets"] = (
                result["combined"]["payload"] <= budgets["payloadBytesPerHour"]
                and result["combined"]["rows"] <= budgets["rowsPerHour"]
                and result["combined"]["wal"] <= budgets["walBytesPerHour"])
            print(json.dumps(result,sort_keys=True),flush=True)


if __name__ == "__main__":
    main()
