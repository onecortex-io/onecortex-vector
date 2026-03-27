from .models import IndexDescription
from ._http import HttpClient
from .index import Index

class Onecortex:
    """
    Main client for the Onecortex Vector API.
    """

    def __init__(self, api_key: str, host: str = "http://localhost:8080"):
        self._http = HttpClient(api_key=api_key, host=host)

    def create_index(
        self,
        name: str,
        dimension: int,
        metric: str = "cosine",
        bm25_enabled: bool = False,
        deletion_protection: str | None = None,
        tags: dict | None = None,
        **kwargs,  # absorb unknown args like spec= without erroring
    ) -> IndexDescription:
        """Create a new vector index."""
        body: dict = {"name": name, "dimension": dimension, "metric": metric}
        if bm25_enabled:
            body["bm25_enabled"] = True
        if deletion_protection:
            body["deletion_protection"] = deletion_protection
        if tags:
            body["tags"] = tags
        # Ignore spec= and other unknown kwargs
        response = self._http.post("/indexes", json=body)
        return IndexDescription.model_validate(response.json())

    def describe_index(self, name: str) -> IndexDescription:
        response = self._http.get(f"/indexes/{name}")
        return IndexDescription.model_validate(response.json())

    def list_indexes(self) -> list[IndexDescription]:
        response = self._http.get("/indexes")
        return [IndexDescription.model_validate(i) for i in response.json().get("indexes", [])]

    def delete_index(self, name: str) -> None:
        self._http.delete(f"/indexes/{name}")

    def configure_index(
        self,
        name: str,
        deletion_protection: str | None = None,
        tags: dict | None = None,
        **kwargs,
    ) -> IndexDescription:
        body: dict = {}
        if deletion_protection is not None:
            body["deletion_protection"] = deletion_protection
        if tags is not None:
            body["tags"] = tags
        response = self._http.patch(f"/indexes/{name}", json=body)
        return IndexDescription.model_validate(response.json())

    def has_index(self, name: str) -> bool:
        try:
            self.describe_index(name)
            return True
        except Exception:
            return False

    def Index(self, name: str) -> Index:
        """Get a handle to a specific index for data-plane operations."""
        return Index(http=self._http, name=name)
