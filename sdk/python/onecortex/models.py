from pydantic import BaseModel, Field, ConfigDict
from typing import Any

class IndexStatus(BaseModel):
    ready: bool
    state: str

class IndexDescription(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    name: str
    dimension: int
    metric: str
    status: IndexStatus
    host: str
    spec: dict = Field(default_factory=dict)
    vector_type: str = "dense"
    tags: dict | None = None

class Match(BaseModel):
    id: str
    score: float
    values: list[float] | None = None
    metadata: dict[str, Any] | None = None

class QueryResult(BaseModel):
    matches: list[Match]
    namespace: str
    results: list = Field(default_factory=list)  # deprecated legacy field

class UpsertResult(BaseModel):
    upserted_count: int = Field(alias="upsertedCount")
    model_config = ConfigDict(populate_by_name=True)

class FetchResult(BaseModel):
    vectors: dict[str, Any]
    namespace: str

class ListResult(BaseModel):
    vectors: list[dict]
    namespace: str
    pagination: dict | None = None

class NamespaceSummary(BaseModel):
    vector_count: int = Field(alias="vectorCount")
    model_config = ConfigDict(populate_by_name=True)

class IndexStats(BaseModel):
    namespaces: dict[str, NamespaceSummary]
    dimension: int
    index_fullness: float = Field(alias="indexFullness")
    total_vector_count: int = Field(alias="totalVectorCount")
    model_config = ConfigDict(populate_by_name=True)
