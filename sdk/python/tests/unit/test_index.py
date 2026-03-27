import pytest
import respx
import httpx
from onecortex import Onecortex
from onecortex.exceptions import InvalidArgumentError


BASE = "http://test-server:8080"
IDX_NAME = "test-idx"
IDX_BASE = f"{BASE}/indexes/{IDX_NAME}"

QUERY_RESPONSE = {
    "matches": [{"id": "v1", "score": 0.99}],
    "namespace": "",
    "results": [],
}

UPSERT_RESPONSE = {"upsertedCount": 2}

FETCH_RESPONSE = {
    "vectors": {
        "v1": {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {}},
    },
    "namespace": "",
}

LIST_RESPONSE = {
    "vectors": [{"id": "v1"}, {"id": "v2"}],
    "namespace": "",
}

STATS_RESPONSE = {
    "namespaces": {"": {"vectorCount": 2}},
    "dimension": 3,
    "indexFullness": 0.001,
    "totalVectorCount": 2,
}


def make_index():
    pc = Onecortex(api_key="key123", host=BASE)
    return pc.Index(IDX_NAME)


@respx.mock
def test_upsert():
    respx.post(f"{IDX_BASE}/vectors/upsert").mock(return_value=httpx.Response(200, json=UPSERT_RESPONSE))
    idx = make_index()
    result = idx.upsert(vectors=[
        {"id": "v1", "values": [1.0, 0.0, 0.0]},
        {"id": "v2", "values": [0.0, 1.0, 0.0]},
    ])
    assert result.upserted_count == 2


@respx.mock
def test_upsert_batch():
    respx.post(f"{IDX_BASE}/vectors/upsert").mock(return_value=httpx.Response(200, json={"upsertedCount": 1}))
    idx = make_index()
    # 3 vectors with batch_size=2 → 2 requests
    total = idx.upsert_batch(
        vectors=[{"id": f"v{i}", "values": [float(i), 0.0, 0.0]} for i in range(3)],
        batch_size=2,
    )
    assert total == 2  # 2 batches × 1 upsertedCount each


@respx.mock
def test_query():
    respx.post(f"{IDX_BASE}/query").mock(return_value=httpx.Response(200, json=QUERY_RESPONSE))
    idx = make_index()
    result = idx.query(vector=[1.0, 0.0, 0.0], top_k=1)
    assert result.matches[0].id == "v1"
    assert result.matches[0].score == 0.99


@respx.mock
def test_query_by_id():
    respx.post(f"{IDX_BASE}/query").mock(return_value=httpx.Response(200, json=QUERY_RESPONSE))
    idx = make_index()
    result = idx.query(vector=[], id="v1", top_k=1)
    assert result.matches[0].id == "v1"


@respx.mock
def test_fetch():
    respx.post(f"{IDX_BASE}/vectors/fetch").mock(return_value=httpx.Response(200, json=FETCH_RESPONSE))
    idx = make_index()
    result = idx.fetch(ids=["v1"])
    assert "v1" in result.vectors


@respx.mock
def test_delete_by_ids():
    respx.post(f"{IDX_BASE}/vectors/delete").mock(return_value=httpx.Response(200, json={}))
    idx = make_index()
    idx.delete(ids=["v1"])  # should not raise


@respx.mock
def test_delete_all():
    respx.post(f"{IDX_BASE}/vectors/delete").mock(return_value=httpx.Response(200, json={}))
    idx = make_index()
    idx.delete(delete_all=True)


def test_delete_no_args_raises():
    idx = make_index()
    with pytest.raises(ValueError):
        idx.delete()


@respx.mock
def test_update():
    respx.post(f"{IDX_BASE}/vectors/update").mock(return_value=httpx.Response(200, json={}))
    idx = make_index()
    idx.update(id="v1", set_metadata={"key": "new"})


@respx.mock
def test_list():
    respx.get(f"{IDX_BASE}/vectors/list").mock(return_value=httpx.Response(200, json=LIST_RESPONSE))
    idx = make_index()
    result = idx.list()
    assert len(result.vectors) == 2


@respx.mock
def test_describe_index_stats():
    respx.post(f"{IDX_BASE}/describe_index_stats").mock(return_value=httpx.Response(200, json=STATS_RESPONSE))
    idx = make_index()
    stats = idx.describe_index_stats()
    assert stats.total_vector_count == 2
    assert stats.dimension == 3


@respx.mock
def test_query_hybrid():
    respx.post(f"{IDX_BASE}/query/hybrid").mock(return_value=httpx.Response(200, json=QUERY_RESPONSE))
    idx = make_index()
    result = idx.query_hybrid(vector=[1.0, 0.0, 0.0], query_text="hello", top_k=5)
    assert result.matches[0].id == "v1"
