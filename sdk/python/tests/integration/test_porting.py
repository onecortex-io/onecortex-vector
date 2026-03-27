"""
Drop-in replacement test: verifies that unknown constructor/method args (e.g. spec=)
are silently ignored and all core operations work correctly.
"""
import pytest

def onecortex_version():
    """Verifies unknown args are silently ignored and core operations work."""
    from onecortex import Onecortex
    pc = Onecortex(api_key="test-api-key-12345", host="http://localhost:8080")
    pc.create_index(name="porting-test", dimension=3, metric="cosine",
                    spec={"serverless": {"cloud": "aws", "region": "us-east-1"}})  # spec= ignored
    idx = pc.Index("porting-test")
    idx.upsert(vectors=[{"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"k": "v"}}])
    results = idx.query(vector=[1.0, 0.0, 0.0], top_k=1, include_metadata=True)
    return results.matches[0].id

@pytest.fixture(autouse=True)
def cleanup(oc_client):
    yield
    try:
        oc_client.delete_index("porting-test")
    except Exception:
        pass

def test_porting(oc_client):
    """The onecortex version of the porting test runs without errors."""
    result = onecortex_version()
    assert result == "v1"
