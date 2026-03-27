import { HttpClient } from './http';
import type {
  UpsertOptions, UpsertResponse, QueryOptions, QueryHybridOptions, QueryResponse,
  FetchOptions, FetchByMetadataOptions, FetchResponse, DeleteOptions, UpdateOptions,
  ListOptions, ListResponse, IndexStats,
} from './types';

export class Index {
  private readonly base: string;

  constructor(private readonly http: HttpClient, private readonly name: string) {
    this.base = `/indexes/${name}`;
  }

  async upsert(options: UpsertOptions): Promise<UpsertResponse> {
    return this.http.post(`${this.base}/vectors/upsert`, options) as Promise<UpsertResponse>;
  }

  async fetch(options: FetchOptions): Promise<FetchResponse> {
    return this.http.post(`${this.base}/vectors/fetch`, options) as Promise<FetchResponse>;
  }

  async fetchByMetadata(options: FetchByMetadataOptions): Promise<FetchResponse> {
    return this.http.post(`${this.base}/vectors/fetch_by_metadata`, options) as Promise<FetchResponse>;
  }

  async delete(options: DeleteOptions): Promise<void> {
    await this.http.post(`${this.base}/vectors/delete`, options);
  }

  async update(options: UpdateOptions): Promise<void> {
    await this.http.post(`${this.base}/vectors/update`, options);
  }

  async query(options: QueryOptions): Promise<QueryResponse> {
    return this.http.post(`${this.base}/query`, {
      topK: options.topK,
      namespace: options.namespace ?? '',
      vector: options.vector,
      id: options.id,
      filter: options.filter,
      includeValues: options.includeValues ?? false,
      includeMetadata: options.includeMetadata ?? true,
    }) as Promise<QueryResponse>;
  }

  /**
   * Onecortex extension: hybrid search combining dense ANN + BM25 keyword ranking.
   * alpha=1.0 is pure vector, alpha=0.0 is pure BM25.
   * Requires index created with bm25_enabled=true.
   * Onecortex extension.
   */
  async queryHybrid(options: QueryHybridOptions): Promise<QueryResponse> {
    return this.http.post(`${this.base}/query/hybrid`, {
      vector: options.vector,
      queryText: options.queryText,
      topK: options.topK,
      alpha: options.alpha ?? 0.7,
      namespace: options.namespace ?? '',
      filter: options.filter,
    }) as Promise<QueryResponse>;
  }

  async list(options: ListOptions = {}): Promise<ListResponse> {
    const params: Record<string, string> = {
      namespace: options.namespace ?? '',
      limit: String(options.limit ?? 100),
    };
    if (options.prefix) params.prefix = options.prefix;
    if (options.paginationToken) params.paginationToken = options.paginationToken;
    return this.http.get(`${this.base}/vectors/list`, params) as Promise<ListResponse>;
  }

  async describeIndexStats(): Promise<IndexStats> {
    return this.http.post(`${this.base}/describe_index_stats`, {}) as Promise<IndexStats>;
  }
}
