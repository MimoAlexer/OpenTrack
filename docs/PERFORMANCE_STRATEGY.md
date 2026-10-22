# Performance and scalability strategy

All numbers are design budgets until a linked benchmark establishes measured results. Current execution evidence is recorded in REQUIREMENTS_STATUS.md. Rust does not automatically make an application fast. [Deployment](DEPLOYMENT_ARCHITECTURE.md), [testing](TESTING_STRATEGY.md), [realtime](REALTIME_ARCHITECTURE.md).

## Reference conditions

Server baseline 4 vCPU/8 GiB RAM/SSD, PostgreSQL 18 and core processes on one machine; agent workloads on separate hosts. Client baseline modern 4-core laptop/8 GiB, current supported Chromium/Firefox,1440×900; browser benchmark uses 4× CPU slowdown and 20 Mbps/50 ms RTT for cold-load reporting. Seed 100k issues,20 teams,100 users,10k projects,1m activity entries,100k comments and 50 GiB blob metadata/storage fixture, with one workspace containing 80% of issues and private-team skew. Record exact machine, versions, filesystem, dataset seed and warm/cold conditions.

Workload:100 connected users,20 concurrently active,30 reads/s and 10 writes/s steady, bursts to 100 reads/s and 30 writes/s for 60s;500 SSE connections for soak/fan-out tests across configured sessions. Use separate 200 writes/s single-workspace stress to locate stream-head contention, not as a promised capacity. External providers are stubbed for core benchmarks and reported separately.

| Metric | Target | Measurement boundary |
| --- | --- | --- |
| API ready after DB available | <=3s p95 | Process start to readiness; excludes image pull/migration |
| Full Compose cold start | <=30s target | Existing images/volumes, no migration backlog |
| Initial page LCP | <=2.5s p75 | Cold browser cache, stated network/CPU profile |
| Initial usable issue list | <=3s p95 | Navigation to operable first page, including auth/session read |
| INP / client interaction | <=200 ms p75; ordinary feedback<=50 ms | Real input to next paint; measure worst large-list interactions |
| Client navigation | <=150 ms p95 warm; <=500 ms uncached | Selection to visible operable page, excluding network outages |
| Issue creation | <=200 ms p95 server; <=500 ms p95 end-to-end | Request received to durable commit response; browser click to confirmed canonical issue |
| Metadata read/write | <=150/200 ms p95 server; <=500 ms p99 | Includes auth/DB pool wait, excludes uploads |
| Common DB statement | <=20 ms p95; <=100 ms p99 | Measured execution separately from pool wait |
| DB pool wait | <=50 ms p95 | Request to acquired connection |
| Search | <=250 ms p95 server; <=500 ms p95 visible result |100k issue relevance/ACL fixture;1s server timeout |
| Realtime | <=500 ms p95 connected; <=3s p95 with lost wakeup | DB commit to authorized client event; poll fallback measured separately |
| Inbox fan-out | <=2s p95 | Commit to durable in-app notification under baseline load |
| Core memory | API<=256 MiB idle/512 MiB load; worker<=256/512 MiB | RSS; PostgreSQL has separate 2 GiB initial budget, not included |
| Core host memory | <=5 GiB steady baseline | All core services plus OS; headroom before optional scanners |
| Browser heap / DOM | <=150 MiB after 30 min; <=150 issue rows mounted |10k-result navigation with virtualization; no unbounded cache growth |
| Initial JS | <=250 KiB gzip target | Shell/list route, excluding lazy editor/agent/diff tools |
| Core worker capacity |4 default concurrent I/O jobs | CPU parsing/hashing separately bounded |
| Agent scheduling | <=5s p95 to claim when capacity free | Queue commit to fenced assignment; excludes VM image cold start |
| Warm runner preparation | <=30s p95 target | Claim to ready environment; checkout size reported separately |
| Tauri cold launch later | <=3s p95 target | Reference desktop, no server startup included |

## Benchmark method and regression gates

Warm up 5 min, run steady 15 min, burst 1 min and recovery 5 min; repeat 3 times. Report p50/p95/p99, error rate, sample size, CPU/RSS, pool wait, DB lock time, event lag and queue oldest age. Run 60 min soak before MVP;24h soak before 1.0 when economical. Trace representative slow requests with redacted IDs. Test cold indexes/cache separately; do not mix them into a single flattering latency average.

Rust profiling: flamegraphs/perf and tracing spans for lock/pool/serialization time; Criterion only for meaningful hot pure functions such as filter compilation if needed. PostgreSQL EXPLAIN ANALYZE/BUFFERS and pg_stat_statements distinguish bad plans, ACL join cost and I/O. Browser Performance/React profiler finds rendering/keystroke work. k6 or equivalent protocol load with Playwright UX probes validates actual service boundaries; choose/install tools only in implementation.

CI runs a small stable performance sample on a controlled runner; release benchmarks use the reference dataset/hardware. Block a confirmed>15% p95 regression or>20% memory/bundle regression against comparable baseline unless the owner documents an accepted tradeoff; investigate noisy results with reruns instead of changing thresholds. Error rate target<0.1% excluding deliberate invalid/denied requests, with zero data-integrity failures.

## Scaling triggers

First optimize query shapes/indexes, eliminate N+1, tune connection budgets and shorten transactions. Add API replicas only after shared storage and per-node replay tests. Add Redis/Valkey wakeups or rate-limit coordination only if measured DB/node overhead warrants another service. Add Tantivy only when tuned PostgreSQL search misses relevance/latency at target size; define index rebuild, ACL enforcement, stale result behavior and per-node distribution first.

If per-workspace stream-head lock consumes>10% write latency or sustained writes miss budget, evaluate CDC/commit-LSN design and preserve replay correctness. If PostgreSQL jobs generate material vacuum/lock pressure, split operational tables/partitions before deploying a broker. Separate a service only for a demonstrated scaling/security/ownership boundary. Agent execution is already separate for security, not imagined throughput.

## Overload and fairness

Bound connection/query/body/stream queues; shed overload with 429/503 and retry hints. A slow SSE consumer disconnects and replays instead of buffering indefinitely. Workspace-fair queues reserve capacity for interactive work and notifications; import/cleanup/agents cannot monopolize the DB or CPU. Benchmarks include one abusive tenant and one normal tenant, proving the latter retains acceptable response and zero cross-tenant leakage.
