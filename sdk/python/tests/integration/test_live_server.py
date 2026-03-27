"""
Integration tests against a live Onecortex Vector server.
Requires: server running on localhost:8080 with a valid API key in ONECORTEX_API_KEY.
"""
import pytest
from onecortex import Onecortex
from onecortex.exceptions import NotFoundError, AlreadyExistsError

INDEX_NAME = "sdk-integration-test"
DIM = 3


@pytest.fixture(autouse=True)
def cleanup(oc_client):
    yield
    try:
        oc_client.delete_index(INDEX_NAME)
    except Exception:
        pass


def test_create_and_describe_index(oc_client):
    idx = oc_client.create_index(name=INDEX_NAME, dimension=DIM, metric="cosine")
    assert idx.name == INDEX_NAME
    assert idx.dimension == DIM

    described = oc_client.describe_index(INDEX_NAME)
    assert described.name == INDEX_NAME


def test_list_indexes(oc_client):
    oc_client.create_index(name=INDEX_NAME, dimension=DIM)
    indexes = oc_client.list_indexes()
    names = [i.name for i in indexes]
    assert INDEX_NAME in names


def test_has_index(oc_client):
    assert oc_client.has_index(INDEX_NAME) is False
    oc_client.create_index(name=INDEX_NAME, dimension=DIM)
    assert oc_client.has_index(INDEX_NAME) is True


def test_upsert_and_fetch(oc_client):
    oc_client.create_index(name=INDEX_NAME, dimension=DIM)
    idx = oc_client.Index(INDEX_NAME)

    result = idx.upsert(vectors=[
        {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"label": "a"}},
        {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"label": "b"}},
    ])
    assert result.upserted_count == 2

    fetched = idx.fetch(ids=["v1"])
    assert "v1" in fetched.vectors


def test_query(oc_client):
    oc_client.create_index(name=INDEX_NAME, dimension=DIM)
    idx = oc_client.Index(INDEX_NAME)
    idx.upsert(vectors=[
        {"id": "v1", "values": [1.0, 0.0, 0.0]},
        {"id": "v2", "values": [0.0, 1.0, 0.0]},
    ])

    results = idx.query(vector=[1.0, 0.0, 0.0], top_k=2, include_metadata=True)
    assert len(results.matches) >= 1
    assert results.matches[0].id == "v1"


def test_delete_by_ids(oc_client):
    oc_client.create_index(name=INDEX_NAME, dimension=DIM)
    idx = oc_client.Index(INDEX_NAME)
    idx.upsert(vectors=[{"id": "v1", "values": [1.0, 0.0, 0.0]}])
    idx.delete(ids=["v1"])

    fetched = idx.fetch(ids=["v1"])
    assert "v1" not in fetched.vectors


def test_describe_index_stats(oc_client):
    oc_client.create_index(name=INDEX_NAME, dimension=DIM)
    idx = oc_client.Index(INDEX_NAME)
    idx.upsert(vectors=[{"id": "v1", "values": [1.0, 0.0, 0.0]}])

    stats = idx.describe_index_stats()
    assert stats.dimension == DIM
    assert stats.total_vector_count >= 1


def test_list_vectors(oc_client):
    oc_client.create_index(name=INDEX_NAME, dimension=DIM)
    idx = oc_client.Index(INDEX_NAME)
    idx.upsert(vectors=[
        {"id": "doc-1", "values": [1.0, 0.0, 0.0]},
        {"id": "doc-2", "values": [0.0, 1.0, 0.0]},
    ])

    result = idx.list(prefix="doc-")
    ids = [v["id"] for v in result.vectors]
    assert "doc-1" in ids
    assert "doc-2" in ids


def test_update_metadata(oc_client):
    oc_client.create_index(name=INDEX_NAME, dimension=DIM)
    idx = oc_client.Index(INDEX_NAME)
    idx.upsert(vectors=[{"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"x": 1}}])
    idx.update(id="v1", set_metadata={"x": 99})

    fetched = idx.fetch(ids=["v1"])
    assert fetched.vectors["v1"]["metadata"]["x"] == 99


def test_not_found_error(oc_client):
    with pytest.raises(NotFoundError):
        oc_client.describe_index("nonexistent-index-xyz")
