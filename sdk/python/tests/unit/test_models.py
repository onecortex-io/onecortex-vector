from onecortex.models import (
    IndexDescription, IndexStatus, Match, QueryResult,
    UpsertResult, FetchResult, ListResult, IndexStats, NamespaceSummary,
)


def test_index_description_defaults():
    data = {
        "name": "my-index",
        "dimension": 1536,
        "metric": "cosine",
        "status": {"ready": True, "state": "Ready"},
        "host": "localhost:8080",
    }
    idx = IndexDescription.model_validate(data)
    assert idx.name == "my-index"
    assert idx.vector_type == "dense"
    assert idx.spec == {}
    assert idx.tags is None


def test_upsert_result_alias():
    result = UpsertResult.model_validate({"upsertedCount": 5})
    assert result.upserted_count == 5


def test_upsert_result_by_name():
    result = UpsertResult.model_validate({"upserted_count": 3})
    assert result.upserted_count == 3


def test_query_result():
    data = {
        "matches": [
            {"id": "v1", "score": 0.95},
            {"id": "v2", "score": 0.80, "metadata": {"key": "val"}},
        ],
        "namespace": "ns1",
    }
    result = QueryResult.model_validate(data)
    assert len(result.matches) == 2
    assert result.matches[0].id == "v1"
    assert result.matches[1].metadata == {"key": "val"}
    assert result.results == []


def test_index_stats_aliases():
    data = {
        "namespaces": {"": {"vectorCount": 10}},
        "dimension": 1536,
        "indexFullness": 0.01,
        "totalVectorCount": 10,
    }
    stats = IndexStats.model_validate(data)
    assert stats.total_vector_count == 10
    assert stats.index_fullness == 0.01
    assert stats.namespaces[""].vector_count == 10


def test_fetch_result():
    data = {
        "vectors": {"v1": {"id": "v1", "values": [0.1, 0.2], "metadata": {}}},
        "namespace": "",
    }
    result = FetchResult.model_validate(data)
    assert "v1" in result.vectors


def test_list_result_no_pagination():
    data = {
        "vectors": [{"id": "v1"}, {"id": "v2"}],
        "namespace": "",
    }
    result = ListResult.model_validate(data)
    assert len(result.vectors) == 2
    assert result.pagination is None
