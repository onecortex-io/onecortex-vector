from .models import QueryResult, UpsertResult, FetchResult, ListResult, IndexStats
from ._http import HttpClient
import math

class Index:
    """
    Handles all data-plane operations for a specific index.
    Handles all data-plane operations for a specific index.
    """

    def __init__(self, http: HttpClient, name: str):
        self._http = http
        self._name = name
        self._base = f"/indexes/{name}"

    def upsert(
        self,
        vectors: list[dict],
        namespace: str = "",
    ) -> UpsertResult:
        """
        Upsert vectors into the index.
        Each vector dict: {"id": str, "values": list[float], "metadata": dict (optional), "text": str (optional)}
        Note: "sparseValues" key is accepted and silently ignored by the server.
        """
        response = self._http.post(
            f"{self._base}/vectors/upsert",
            json={"vectors": vectors, "namespace": namespace},
        )
        return UpsertResult.model_validate(response.json())

    def upsert_batch(
        self,
        vectors: list[dict],
        namespace: str = "",
        batch_size: int = 200,
    ) -> int:
        """
        Upsert a large list of vectors in batches.
        Returns total upserted count.
        """
        total = 0
        for i in range(0, len(vectors), batch_size):
            batch = vectors[i : i + batch_size]
            result = self.upsert(batch, namespace=namespace)
            total += result.upserted_count
        return total

    def fetch(
        self,
        ids: list[str],
        namespace: str = "",
    ) -> FetchResult:
        """Fetch vectors by ID."""
        response = self._http.post(
            f"{self._base}/vectors/fetch",
            json={"ids": ids, "namespace": namespace},
        )
        return FetchResult.model_validate(response.json())

    def fetch_by_metadata(
        self,
        filter: dict,
        namespace: str = "",
        limit: int = 100,
        include_values: bool = False,
        include_metadata: bool = True,
    ) -> FetchResult:
        """Fetch vectors matching a metadata filter (Onecortex extension)."""
        response = self._http.post(
            f"{self._base}/vectors/fetch_by_metadata",
            json={
                "filter": filter,
                "namespace": namespace,
                "limit": limit,
                "include_values": include_values,
                "include_metadata": include_metadata,
            },
        )
        return FetchResult.model_validate(response.json())

    def delete(
        self,
        ids: list[str] | None = None,
        filter: dict | None = None,
        delete_all: bool = False,
        namespace: str = "",
    ) -> None:
        """Delete vectors by IDs, by metadata filter, or all in namespace."""
        body: dict = {"namespace": namespace}
        if delete_all:
            body["deleteAll"] = True
        elif ids is not None:
            body["ids"] = ids
        elif filter is not None:
            body["filter"] = filter
        else:
            raise ValueError("Provide ids, filter, or delete_all=True")
        self._http.post(f"{self._base}/vectors/delete", json=body)

    def update(
        self,
        id: str,
        values: list[float] | None = None,
        set_metadata: dict | None = None,
        text: str | None = None,
        namespace: str = "",
    ) -> None:
        """Update values and/or metadata for a single vector. Metadata is merged, not replaced."""
        body: dict = {"id": id, "namespace": namespace}
        if values is not None:
            body["values"] = values
        if set_metadata is not None:
            body["setMetadata"] = set_metadata
        if text is not None:
            body["text"] = text
        self._http.post(f"{self._base}/vectors/update", json=body)

    def query(
        self,
        vector: list[float],
        top_k: int = 10,
        namespace: str = "",
        filter: dict | None = None,
        include_values: bool = False,
        include_metadata: bool = True,
        id: str | None = None,
        rerank: dict | None = None,
    ) -> QueryResult:
        """
        Search for similar vectors using dense ANN.
        Note: euclidean metric scores use 1/(1+distance) normalization.

        Args:
            rerank: Optional reranking options dict. Keys:
                query (str, required): Natural-language query for the reranker.
                topN (int, optional): Number of results after reranking. Defaults to top_k.
                rankField (str, optional): Metadata field to rank against. Defaults to "text".
                model (str, optional): Per-request model override for the reranker backend.
        """
        body: dict = {
            "topK": top_k,
            "namespace": namespace,
            "includeValues": include_values,
            "includeMetadata": include_metadata,
        }
        if id is not None:
            body["id"] = id
        else:
            body["vector"] = vector
        if filter is not None:
            body["filter"] = filter
        if rerank is not None:
            body["rerank"] = rerank
        response = self._http.post(f"{self._base}/query", json=body)
        return QueryResult.model_validate(response.json())

    def query_hybrid(
        self,
        vector: list[float],
        text: str,
        top_k: int = 10,
        alpha: float = 0.5,
        namespace: str = "",
        filter: dict | None = None,
        include_metadata: bool = False,
        include_values: bool = False,
        rerank: dict | None = None,
    ) -> QueryResult:
        """
        Hybrid ANN + BM25 query with Reciprocal Rank Fusion.

        Requires bm25_enabled=True on the index. Use the client's
        configure_index(name, bm25_enabled=True) to enable it.

        Args:
            vector: Dense query vector (must match index dimension).
            text:   BM25 query text.
            top_k:  Number of results to return (max 10000).
            alpha:  Dense weight [0.0, 1.0]. 0.5 = equal blend.
                    1.0 = pure dense, 0.0 = pure BM25.
            filter: Metadata filter (same DSL as query()).
            namespace: Namespace to search within.
            include_metadata: Include metadata in results.
            include_values:   Include vector values in results.
            rerank: Optional reranking options dict. Keys:
                query (str, required): Natural-language query for the reranker.
                topN (int, optional): Number of results after reranking. Defaults to top_k.
                rankField (str, optional): Metadata field to rank against. Defaults to "text".
                model (str, optional): Per-request model override for the reranker backend.

        Returns:
            QueryResult with matches and rrf_score as the score field.
        """
        body: dict = {
            "vector": vector,
            "text": text,
            "topK": top_k,
            "alpha": alpha,
            "namespace": namespace,
            "includeMetadata": include_metadata,
            "includeValues": include_values,
        }
        if filter is not None:
            body["filter"] = filter
        if rerank is not None:
            body["rerank"] = rerank
        response = self._http.post(f"{self._base}/query/hybrid", json=body)
        return QueryResult.model_validate(response.json())

    def list(
        self,
        namespace: str = "",
        prefix: str = "",
        limit: int = 100,
        pagination_token: str | None = None,
    ) -> ListResult:
        """List vector IDs in a namespace, optionally filtered by prefix."""
        params: dict = {"namespace": namespace, "limit": str(limit)}
        if prefix:
            params["prefix"] = prefix
        if pagination_token:
            params["paginationToken"] = pagination_token
        response = self._http.get(f"{self._base}/vectors/list", params=params)
        return ListResult.model_validate(response.json())

    def describe_index_stats(self) -> IndexStats:
        """Get vector counts per namespace."""
        response = self._http.post(f"{self._base}/describe_index_stats", json={})
        return IndexStats.model_validate(response.json())
