import pytest
import respx
import httpx
from onecortex import Onecortex
from onecortex.exceptions import NotFoundError


BASE = "http://test-server:8080"

INDEX_RESPONSE = {
    "name": "test-idx",
    "dimension": 3,
    "metric": "cosine",
    "status": {"ready": True, "state": "Ready"},
    "host": "test-server:8080",
}


@respx.mock
def test_create_index():
    respx.post(f"{BASE}/indexes").mock(return_value=httpx.Response(200, json=INDEX_RESPONSE))
    pc = Onecortex(api_key="key123", host=BASE)
    idx = pc.create_index(name="test-idx", dimension=3, metric="cosine")
    assert idx.name == "test-idx"
    assert idx.dimension == 3


@respx.mock
def test_create_index_ignores_spec():
    respx.post(f"{BASE}/indexes").mock(return_value=httpx.Response(200, json=INDEX_RESPONSE))
    pc = Onecortex(api_key="key123", host=BASE)
    # spec= is an unknown arg — must not raise
    idx = pc.create_index(
        name="test-idx", dimension=3,
        spec={"serverless": {"cloud": "aws", "region": "us-east-1"}},
    )
    assert idx.name == "test-idx"


@respx.mock
def test_describe_index():
    respx.get(f"{BASE}/indexes/test-idx").mock(return_value=httpx.Response(200, json=INDEX_RESPONSE))
    pc = Onecortex(api_key="key123", host=BASE)
    idx = pc.describe_index("test-idx")
    assert idx.metric == "cosine"


@respx.mock
def test_list_indexes():
    respx.get(f"{BASE}/indexes").mock(
        return_value=httpx.Response(200, json={"indexes": [INDEX_RESPONSE]})
    )
    pc = Onecortex(api_key="key123", host=BASE)
    indexes = pc.list_indexes()
    assert len(indexes) == 1
    assert indexes[0].name == "test-idx"


@respx.mock
def test_delete_index():
    respx.delete(f"{BASE}/indexes/test-idx").mock(return_value=httpx.Response(202))
    pc = Onecortex(api_key="key123", host=BASE)
    pc.delete_index("test-idx")  # should not raise


@respx.mock
def test_has_index_true():
    respx.get(f"{BASE}/indexes/test-idx").mock(return_value=httpx.Response(200, json=INDEX_RESPONSE))
    pc = Onecortex(api_key="key123", host=BASE)
    assert pc.has_index("test-idx") is True


@respx.mock
def test_has_index_false():
    respx.get(f"{BASE}/indexes/missing").mock(
        return_value=httpx.Response(404, json={"error": {"code": "NOT_FOUND", "message": "not found"}})
    )
    pc = Onecortex(api_key="key123", host=BASE)
    assert pc.has_index("missing") is False


@respx.mock
def test_configure_index():
    respx.patch(f"{BASE}/indexes/test-idx").mock(return_value=httpx.Response(200, json=INDEX_RESPONSE))
    pc = Onecortex(api_key="key123", host=BASE)
    result = pc.configure_index("test-idx", tags={"env": "prod"})
    assert result.name == "test-idx"


@respx.mock
def test_not_found_raises():
    respx.get(f"{BASE}/indexes/missing").mock(
        return_value=httpx.Response(404, json={"error": {"code": "NOT_FOUND", "message": "not found"}})
    )
    pc = Onecortex(api_key="key123", host=BASE)
    with pytest.raises(NotFoundError):
        pc.describe_index("missing")
