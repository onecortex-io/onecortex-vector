#!/usr/bin/env python3
"""
Onecortex Vector — Load Test
=======================================
1. Creates an API key via the admin API (port 9090).
2. Creates a BM25-enabled collection with 1536-dimensional cosine vectors.
3. Pre-generates semantic topic centroids to simulate clustered embedding spaces.
4. Bulk-upserts TOTAL_RECORDS vectors in batches of UPSERT_BATCH_SIZE.
   Each record uses a cluster-perturbed float32-precision vector, paragraph-length
   domain-coherent text, and realistic metadata (author, timestamps, tags, etc.).
5. Runs QUERY_CONCURRENCY concurrent hybrid queries for QUERY_DURATION_S seconds.
   40% of queries include a metadata filter to exercise the filter code path.
6. Prints a latency/throughput summary.

Usage:
    python3 scripts/load_test.py
    python3 scripts/load_test.py --records 100000 --concurrency 32
    python3 scripts/load_test.py --records 1000 --duration 10   # quick smoke test
"""

import argparse
import datetime
import json
import math
import random
import statistics
import threading
import time
import urllib.error
import urllib.request

# ── defaults ─────────────────────────────────────────────────────────────────
DEFAULT_BASE_URL = "http://localhost:8080"
DEFAULT_ADMIN_URL = "http://localhost:9090"
DEFAULT_COLLECTION = "load-test-col"
DEFAULT_DIMENSION = 1536
# 1536-dim vectors: ~15.7 KB per record in JSON → 100 records ≈ 1.57 MB (safe under axum 2 MB limit)
DEFAULT_UPSERT_BATCH = 100
DEFAULT_CONCURRENCY = 16  # concurrent hybrid-query workers
DEFAULT_QUERY_SECS = 30  # how long to hammer queries
DEFAULT_TOP_K = 10
DEFAULT_TOPICS = 50  # semantic cluster centroids
DEFAULT_TOTAL_RECORDS = 50_000

# ── domain vocabulary & metadata corpus ──────────────────────────────────────
DOMAIN_VOCAB = {
    "technology": {
        "vocab": [
            "algorithm",
            "latency",
            "throughput",
            "inference",
            "deployment",
            "containerization",
            "microservice",
            "orchestration",
            "sharding",
            "replication",
        ],
        "tags": ["ml", "infrastructure", "cloud", "api", "devops", "performance"],
    },
    "science": {
        "vocab": [
            "hypothesis",
            "experiment",
            "observation",
            "membrane",
            "catalyst",
            "equilibrium",
            "diffusion",
            "genome",
            "spectroscopy",
            "thermodynamics",
        ],
        "tags": [
            "research",
            "biology",
            "chemistry",
            "physics",
            "peer-reviewed",
            "data",
        ],
    },
    "finance": {
        "vocab": [
            "liquidity",
            "arbitrage",
            "derivative",
            "amortization",
            "collateral",
            "volatility",
            "portfolio",
            "benchmark",
            "leverage",
            "yield",
        ],
        "tags": [
            "investing",
            "risk",
            "markets",
            "equities",
            "fixed-income",
            "compliance",
        ],
    },
    "healthcare": {
        "vocab": [
            "diagnosis",
            "prognosis",
            "etiology",
            "pharmacokinetics",
            "biomarker",
            "pathology",
            "clinical",
            "protocol",
            "intervention",
            "epidemiology",
        ],
        "tags": [
            "medical",
            "clinical-trial",
            "patient-care",
            "pharma",
            "public-health",
            "fda",
        ],
    },
    "legal": {
        "vocab": [
            "jurisdiction",
            "precedent",
            "litigation",
            "indemnification",
            "arbitration",
            "fiduciary",
            "statute",
            "compliance",
            "injunction",
            "liability",
        ],
        "tags": ["contract", "regulatory", "ip", "employment", "corporate", "dispute"],
    },
}

DOMAIN_NAMES = list(DOMAIN_VOCAB.keys())
CONTENT_TYPES = ["article", "documentation", "product", "research", "support"]

AUTHORS = [
    "Dr. Sarah Chen",
    "James Okafor",
    "Maria Gonzalez",
    "Wei Zhang",
    "Priya Patel",
    "Thomas Müller",
    "Aisha Adeyemi",
    "Luca Rossi",
    "Emma Johansson",
    "Raj Krishnamurthy",
    "Claire Dubois",
    "Kwame Mensah",
    "Yuki Tanaka",
    "Ana Sousa",
    "Ben Harrington",
    "Fatima Al-Rashid",
    "Ivan Petrov",
    "Zoe Williams",
    "Omar Hassan",
    "Mei Lin",
]

QUERY_TEMPLATES = [
    "What are the best practices for {topic} in modern {domain} systems?",
    "How does {topic} affect performance and scalability?",
    "Explain the relationship between {topic} and {topic2} in {domain}.",
    "What are the key challenges when implementing {topic} at scale?",
    "How can {topic} be optimized for production {domain} workloads?",
    "What is the impact of {topic} on {domain} outcomes?",
    "Compare different approaches to {topic} in the context of {domain}.",
    "Why is {topic} considered critical for {domain} applications?",
    "What metrics should be used to evaluate {topic} effectiveness?",
    "How do {topic} and {topic2} interact in distributed {domain} environments?",
    "What are the trade-offs between {topic} and {topic2}?",
    "Describe a common failure mode related to {topic} in {domain}.",
    "How has {topic} evolved over the past few years in {domain}?",
    "What tools and frameworks support {topic} for {domain} practitioners?",
    "When should {topic} be preferred over alternative approaches in {domain}?",
]

_SENT_TEMPLATES = [
    "The {adj} approach to {topic} enables more effective {action} in {domain} workflows.",
    "Recent advances in {topic} have demonstrated significant improvements in {action}.",
    "When evaluating {topic}, practitioners must consider {adj} factors such as {action} and scalability.",
    "The {adj} relationship between {topic} and {topic2} is central to understanding {domain}.",
    "Organizations adopting {topic} report improvements in {action} and overall system reliability.",
    "A {adj} implementation of {topic} requires careful attention to {action} and resource constraints.",
    "In {domain}, {topic} serves as a foundational mechanism for enabling {action} at scale.",
    "The integration of {topic} with existing {domain} infrastructure poses {adj} challenges.",
    "Effective {action} depends on a thorough understanding of {topic} principles.",
    "Studies show that {topic} contributes directly to improved {action} outcomes in {domain}.",
]

_ADJECTIVES = [
    "robust",
    "scalable",
    "efficient",
    "distributed",
    "modular",
    "adaptive",
    "fault-tolerant",
    "high-performance",
    "cost-effective",
    "production-ready",
]

_ACTIONS = [
    "data processing",
    "retrieval",
    "classification",
    "monitoring",
    "deployment",
    "optimization",
    "indexing",
    "filtering",
    "aggregation",
    "orchestration",
]

_TS_START = datetime.datetime(2023, 1, 1, tzinfo=datetime.timezone.utc)
_TS_RANGE_SECS = int(
    (
        datetime.datetime(2025, 12, 31, 23, 59, 59, tzinfo=datetime.timezone.utc)
        - _TS_START
    ).total_seconds()
)


# ── helpers ───────────────────────────────────────────────────────────────────


def post(url: str, body: dict, api_key: str | None = None) -> dict:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            **({"Api-Key": api_key} if api_key else {}),
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def _raw_rand_unit_vec(dim: int) -> list[float]:
    v = [random.gauss(0, 1) for _ in range(dim)]
    norm = math.sqrt(sum(x * x for x in v)) or 1.0
    return [x / norm for x in v]


def init_centroids(n: int, dim: int) -> list[list[float]]:
    """Pre-generate n unit-norm centroid vectors representing topic cluster centers."""
    return [_raw_rand_unit_vec(dim) for _ in range(n)]


def make_record_vec(centroid: list[float], sigma: float = 0.1) -> list[float]:
    """
    Perturb a centroid with Gaussian noise, then normalize and truncate to
    float32 precision (~6 significant digits). Produces realistic embedding
    vectors clustered around a semantic topic center.
    """
    v = [c + random.gauss(0, sigma) for c in centroid]
    norm = math.sqrt(sum(x * x for x in v)) or 1.0
    v = [float(f"{x / norm:.6g}") for x in v]
    norm2 = math.sqrt(sum(x * x for x in v)) or 1.0
    return [float(f"{x / norm2:.6g}") for x in v]


def rand_vec(dim: int) -> list[float]:
    """Fully random unit-norm query vector with float32 precision truncation."""
    v = [random.gauss(0, 1) for _ in range(dim)]
    norm = math.sqrt(sum(x * x for x in v)) or 1.0
    return [float(f"{x / norm:.6g}") for x in v]


def rand_text(
    domain: str | None = None, min_words: int = 60, max_words: int = 120
) -> str:
    """Generate a paragraph-length, domain-coherent document body."""
    if domain is None:
        domain = random.choice(DOMAIN_NAMES)
    vocab = DOMAIN_VOCAB[domain]["vocab"]
    target_words = random.randint(min_words, max_words)
    sentences: list[str] = []
    word_count = 0
    while word_count < target_words:
        sentence = random.choice(_SENT_TEMPLATES).format(
            adj=random.choice(_ADJECTIVES),
            topic=random.choice(vocab),
            topic2=random.choice(vocab),
            action=random.choice(_ACTIONS),
            domain=domain,
        )
        sentences.append(sentence)
        word_count += len(sentence.split())
    return " ".join(sentences)


def rand_metadata(domain: str, record_id: str) -> dict:
    """Generate realistic document metadata matching modern AI application patterns."""
    lang_roll = random.random()
    if lang_roll < 0.95:
        language = "en"
    elif lang_roll < 0.9667:
        language = "es"
    elif lang_roll < 0.9833:
        language = "fr"
    else:
        language = "de"

    published_at = (
        _TS_START + datetime.timedelta(seconds=random.randint(0, _TS_RANGE_SECS))
    ).strftime("%Y-%m-%dT%H:%M:%SZ")

    return {
        "source_url": f"https://docs.example.com/articles/{record_id}",
        "author": random.choice(AUTHORS),
        "published_at": published_at,
        "word_count": random.randint(50, 800),
        "language": language,
        "tags": random.sample(DOMAIN_VOCAB[domain]["tags"], k=random.randint(1, 4)),
        "content_type": random.choice(CONTENT_TYPES),
        "is_published": random.random() < 0.90,
        "relevance_score": round(random.uniform(0.0, 1.0), 4),
        "category": domain,
    }


def rand_query_text(domain: str | None = None) -> str:
    """Generate a natural language search query for the given domain."""
    if domain is None:
        domain = random.choice(DOMAIN_NAMES)
    vocab = DOMAIN_VOCAB[domain]["vocab"]
    return random.choice(QUERY_TEMPLATES).format(
        topic=random.choice(vocab),
        topic2=random.choice(vocab),
        domain=domain,
    )


def rand_filter() -> dict | None:
    """Return a metadata filter 40% of the time, rotating among scalar field predicates."""
    if random.random() > 0.40:
        return None
    roll = random.random()
    if roll < 0.33:
        return {"content_type": {"$eq": random.choice(CONTENT_TYPES)}}
    elif roll < 0.66:
        return {"category": {"$eq": random.choice(DOMAIN_NAMES)}}
    else:
        return {"relevance_score": {"$gte": round(random.uniform(0.5, 0.9), 2)}}


def fmt_ms(ms: float) -> str:
    return f"{ms:.1f} ms"


# ── phases ────────────────────────────────────────────────────────────────────


def create_api_key(admin_url: str) -> str:
    print("▶ Creating API key via admin API …")
    resp = post(f"{admin_url}/admin/api_keys", {"name": "load-test-key"})
    key = resp["key"]
    print(f"  key = {key[:12]}…")
    return key


def create_collection(base_url: str, api_key: str, name: str, dim: int) -> None:
    print(f"▶ Creating collection '{name}' (dim={dim}, metric=cosine, bm25=true) …")
    req = urllib.request.Request(
        f"{base_url}/collections",
        data=json.dumps(
            {"name": name, "dimension": dim, "metric": "cosine", "bm25_enabled": True}
        ).encode(),
        headers={"Content-Type": "application/json", "Api-Key": api_key},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            status = resp.status
    except urllib.error.HTTPError as e:
        if e.code == 409:
            print("  collection already exists — reusing it")
            return
        raise
    print(f"  status = {status}")


def bulk_upsert(
    base_url: str,
    api_key: str,
    name: str,
    total: int,
    batch_size: int,
    dim: int,
    centroids: list[list[float]],
) -> None:
    print(f"▶ Upserting {total:,} records in batches of {batch_size} …")
    url = f"{base_url}/collections/{name}/records/upsert"
    batches = math.ceil(total / batch_size)
    centroid_domains = [
        DOMAIN_NAMES[j % len(DOMAIN_NAMES)] for j in range(len(centroids))
    ]
    t0 = time.perf_counter()
    upserted = 0

    for b in range(batches):
        start_id = b * batch_size
        end_id = min(start_id + batch_size, total)
        records = []
        for i in range(start_id, end_id):
            ci = random.randrange(len(centroids))
            domain = centroid_domains[ci]
            rec_id = f"rec-{i:07d}"
            records.append(
                {
                    "id": rec_id,
                    "values": make_record_vec(centroids[ci]),
                    "text": rand_text(domain),
                    "metadata": rand_metadata(domain, rec_id),
                }
            )
        resp = post(url, {"records": records}, api_key)
        upserted += resp.get("upsertedCount", 0)
        elapsed = time.perf_counter() - t0
        rate = upserted / elapsed
        pct = 100 * upserted / total
        print(
            f"  {pct:5.1f}%  {upserted:,}/{total:,} records  ({rate:,.0f} rec/s)",
            end="\r",
            flush=True,
        )

    elapsed = time.perf_counter() - t0
    rate = upserted / elapsed
    print(f"\n  Done. {upserted:,} records in {elapsed:.1f}s  ({rate:,.0f} rec/s)")


def query_worker(
    base_url: str,
    api_key: str,
    name: str,
    dim: int,
    top_k: int,
    stop_event: threading.Event,
    results: list,
    centroids: list[list[float]],
    centroid_domains: list[str],
) -> None:
    url = f"{base_url}/collections/{name}/query/hybrid"
    while not stop_event.is_set():
        ci = random.randrange(len(centroids))
        domain = centroid_domains[ci]
        body: dict = {
            "vector": make_record_vec(centroids[ci], sigma=0.15),
            "text": rand_query_text(domain),
            "topK": top_k,
            "alpha": round(random.uniform(0.3, 0.7), 2),
        }
        qf = rand_filter()
        if qf is not None:
            body["filter"] = qf
            body["includeMetadata"] = True
        t0 = time.perf_counter()
        try:
            post(url, body, api_key)
            latency_ms = (time.perf_counter() - t0) * 1000
            results.append(("ok", latency_ms))
        except Exception as exc:
            results.append(("err", str(exc)))


def run_query_load(
    base_url: str,
    api_key: str,
    name: str,
    dim: int,
    top_k: int,
    concurrency: int,
    duration_s: int,
    centroids: list[list[float]],
    centroid_domains: list[str],
) -> None:
    print(f"▶ Running hybrid query load: {concurrency} workers × {duration_s}s …")
    stop_event = threading.Event()
    all_results: list = []
    lock = threading.Lock()

    def worker_wrapper():
        local: list = []
        query_worker(
            base_url,
            api_key,
            name,
            dim,
            top_k,
            stop_event,
            local,
            centroids,
            centroid_domains,
        )
        with lock:
            all_results.extend(local)

    threads = [
        threading.Thread(target=worker_wrapper, daemon=True) for _ in range(concurrency)
    ]
    t0 = time.perf_counter()
    for t in threads:
        t.start()

    for remaining in range(duration_s, 0, -1):
        time.sleep(1)
        with lock:
            n = sum(1 for r in all_results if r[0] == "ok")
        elapsed = time.perf_counter() - t0
        qps = n / elapsed if elapsed > 0 else 0
        print(
            f"  {remaining:2d}s left  |  {n:,} queries  |  {qps:.1f} QPS",
            end="\r",
            flush=True,
        )

    stop_event.set()
    for t in threads:
        t.join(timeout=5)

    print()  # newline after \r

    ok_latencies = [r[1] for r in all_results if r[0] == "ok"]
    errors = [r for r in all_results if r[0] == "err"]

    if not ok_latencies:
        print("  No successful queries — check server logs.")
        return

    ok_latencies.sort()
    total_queries = len(ok_latencies)
    elapsed = time.perf_counter() - t0
    qps = total_queries / elapsed

    def pct(p: float) -> float:
        idx = min(int(p / 100 * total_queries), total_queries - 1)
        return ok_latencies[idx]

    print()
    print("═" * 52)
    print("  HYBRID QUERY LOAD TEST RESULTS")
    print("═" * 52)
    print(f"  Duration          : {elapsed:.1f}s")
    print(f"  Concurrency       : {concurrency} workers")
    print(f"  Total queries     : {total_queries:,}")
    print(f"  Errors            : {len(errors):,}")
    print(f"  Throughput        : {qps:.1f} QPS")
    print(f"  Mean latency      : {fmt_ms(statistics.mean(ok_latencies))}")
    print(f"  Median (p50)      : {fmt_ms(pct(50))}")
    print(f"  p75               : {fmt_ms(pct(75))}")
    print(f"  p90               : {fmt_ms(pct(90))}")
    print(f"  p95               : {fmt_ms(pct(95))}")
    print(f"  p99               : {fmt_ms(pct(99))}")
    print(f"  Min               : {fmt_ms(ok_latencies[0])}")
    print(f"  Max               : {fmt_ms(ok_latencies[-1])}")
    print("═" * 52)

    if errors:
        print(f"\n  First 5 errors:")
        for _, msg in errors[:5]:
            print(f"    {msg}")


def delete_collection(base_url: str, api_key: str, name: str) -> None:
    req = urllib.request.Request(
        f"{base_url}/collections/{name}",
        headers={"Api-Key": api_key},
        method="DELETE",
    )
    try:
        with urllib.request.urlopen(req, timeout=10):
            pass
        print(f"▶ Collection '{name}' deleted.")
    except Exception as e:
        print(f"▶ Could not delete collection: {e}")


# ── main ──────────────────────────────────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(description="Onecortex Vector load test")
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL)
    parser.add_argument("--admin-url", default=DEFAULT_ADMIN_URL)
    parser.add_argument("--collection", default=DEFAULT_COLLECTION)
    parser.add_argument("--dimension", type=int, default=DEFAULT_DIMENSION)
    parser.add_argument("--records", type=int, default=DEFAULT_TOTAL_RECORDS)
    parser.add_argument("--batch-size", type=int, default=DEFAULT_UPSERT_BATCH)
    parser.add_argument("--concurrency", type=int, default=DEFAULT_CONCURRENCY)
    parser.add_argument(
        "--duration",
        type=int,
        default=DEFAULT_QUERY_SECS,
        help="Query phase duration in seconds",
    )
    parser.add_argument("--top-k", type=int, default=DEFAULT_TOP_K)
    parser.add_argument(
        "--topics",
        type=int,
        default=DEFAULT_TOPICS,
        help="Number of semantic topic centroids for cluster-based vector generation",
    )
    parser.add_argument(
        "--skip-load",
        action="store_true",
        help="Skip upsert phase (use existing collection)",
    )
    parser.add_argument(
        "--keep", action="store_true", help="Keep the collection after the test"
    )
    args = parser.parse_args()

    print()
    print("Onecortex Vector — Load Test")
    print(f"  base  : {args.base_url}")
    print(f"  admin : {args.admin_url}")
    print(
        f"  dim   : {args.dimension}  records: {args.records:,}  "
        f"batch: {args.batch_size}  concurrency: {args.concurrency}  topics: {args.topics}"
    )
    print()

    print("Initializing semantic centroids …", end=" ", flush=True)
    centroids = init_centroids(args.topics, args.dimension)
    centroid_domains = [DOMAIN_NAMES[j % len(DOMAIN_NAMES)] for j in range(args.topics)]
    print("done.")
    print()

    api_key = create_api_key(args.admin_url)

    if not args.skip_load:
        create_collection(args.base_url, api_key, args.collection, args.dimension)
        bulk_upsert(
            args.base_url,
            api_key,
            args.collection,
            args.records,
            args.batch_size,
            args.dimension,
            centroids,
        )

    run_query_load(
        args.base_url,
        api_key,
        args.collection,
        args.dimension,
        args.top_k,
        args.concurrency,
        args.duration,
        centroids,
        centroid_domains,
    )

    if not args.keep:
        delete_collection(args.base_url, api_key, args.collection)


if __name__ == "__main__":
    main()
