export interface IndexDescription {
  name: string;
  dimension: number;
  metric: 'cosine' | 'euclidean' | 'dotproduct';
  status: { ready: boolean; state: string };
  host: string;
  spec: Record<string, unknown>;
  vector_type: string;
  tags?: Record<string, string>;
}

export interface CreateIndexOptions {
  name: string;
  dimension: number;
  metric?: 'cosine' | 'euclidean' | 'dotproduct';
  bm25_enabled?: boolean;
  deletion_protection?: 'enabled' | 'disabled';
  tags?: Record<string, string>;
  spec?: unknown; // accepted and ignored for compatibility
}

export interface Vector {
  id: string;
  values: number[];
  sparseValues?: { indices: number[]; values: number[] }; // accepted, silently ignored
  metadata?: Record<string, unknown>;
  text?: string; // for BM25 hybrid search
}

export interface UpsertOptions {
  vectors: Vector[];
  namespace?: string;
}

export interface UpsertResponse {
  upsertedCount: number;
}

export interface Match {
  id: string;
  score: number;
  values?: number[];
  metadata?: Record<string, unknown>;
}

export interface QueryResponse {
  matches: Match[];
  namespace: string;
  results: unknown[]; // deprecated legacy field
}

export interface QueryOptions {
  vector?: number[];
  id?: string;
  topK: number;
  namespace?: string;
  filter?: Record<string, unknown>;
  includeValues?: boolean;
  includeMetadata?: boolean;
}

export interface QueryHybridOptions {
  vector: number[];
  queryText: string;
  topK: number;
  alpha?: number;
  namespace?: string;
  filter?: Record<string, unknown>;
}

export interface FetchOptions {
  ids: string[];
  namespace?: string;
}

export interface FetchByMetadataOptions {
  filter: Record<string, unknown>;
  namespace?: string;
  limit?: number;
  includeValues?: boolean;
  includeMetadata?: boolean;
}

export interface FetchResponse {
  vectors: Record<string, Vector>;
  namespace: string;
}

export interface DeleteOptions {
  ids?: string[];
  filter?: Record<string, unknown>;
  deleteAll?: boolean;
  namespace?: string;
}

export interface UpdateOptions {
  id: string;
  values?: number[];
  setMetadata?: Record<string, unknown>;
  text?: string;
  namespace?: string;
}

export interface ListOptions {
  namespace?: string;
  prefix?: string;
  limit?: number;
  paginationToken?: string;
}

export interface ListResponse {
  vectors: { id: string }[];
  namespace: string;
  pagination?: { next?: string };
}

export interface IndexStats {
  namespaces: Record<string, { vectorCount: number }>;
  dimension: number;
  indexFullness: number;
  totalVectorCount: number;
}

export interface OnecortexConfig {
  apiKey: string;
  host?: string;
}
