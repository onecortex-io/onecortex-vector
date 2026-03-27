import { HttpClient } from './http';
import { Index } from './index-client';
import type { OnecortexConfig, CreateIndexOptions, IndexDescription } from './types';

export class Onecortex {
  private readonly http: HttpClient;

  /** Create an Onecortex client. */
  constructor(config: OnecortexConfig) {
    this.http = new HttpClient(config);
  }

  async createIndex(options: CreateIndexOptions): Promise<IndexDescription> {
    return this.http.post('/indexes', {
      name: options.name,
      dimension: options.dimension,
      metric: options.metric ?? 'cosine',
      bm25_enabled: options.bm25_enabled,
      deletion_protection: options.deletion_protection,
      tags: options.tags,
      // spec is accepted and ignored
    }) as Promise<IndexDescription>;
  }

  async describeIndex(name: string): Promise<IndexDescription> {
    return this.http.get(`/indexes/${name}`) as Promise<IndexDescription>;
  }

  async listIndexes(): Promise<{ indexes: IndexDescription[] }> {
    return this.http.get('/indexes') as Promise<{ indexes: IndexDescription[] }>;
  }

  async deleteIndex(name: string): Promise<void> {
    await this.http.delete(`/indexes/${name}`);
  }

  async configureIndex(
    name: string,
    options: { deletion_protection?: 'enabled' | 'disabled'; tags?: Record<string, string> },
  ): Promise<IndexDescription> {
    return this.http.patch(`/indexes/${name}`, options) as Promise<IndexDescription>;
  }

  /** Get a handle to a specific index for data-plane operations. */
  index(name: string): Index {
    return new Index(this.http, name);
  }
}
