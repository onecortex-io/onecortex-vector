/**
 * Integration tests against a live Onecortex Vector server.
 * Requires: server running on localhost:8080 with a valid API key in ONECORTEX_API_KEY.
 *
 * Run with: npm run test:integration
 */
import { describe, it, expect, afterEach } from 'vitest';
import { Onecortex } from '../../src/client';
import { OnecortexHttpError } from '../../src/http';

const HOST = process.env.ONECORTEX_HOST ?? 'http://localhost:8080';
const API_KEY = process.env.ONECORTEX_API_KEY ?? '';
const INDEX_NAME = 'ts-sdk-integration-test';
const DIM = 3;

const pc = new Onecortex({ apiKey: API_KEY, host: HOST });

afterEach(async () => {
  try {
    await pc.deleteIndex(INDEX_NAME);
  } catch {}
});

describe('Live server integration', () => {
  it('create and describe index', async () => {
    const idx = await pc.createIndex({ name: INDEX_NAME, dimension: DIM, metric: 'cosine' });
    expect(idx.name).toBe(INDEX_NAME);
    expect(idx.dimension).toBe(DIM);

    const described = await pc.describeIndex(INDEX_NAME);
    expect(described.name).toBe(INDEX_NAME);
  });

  it('list indexes includes created index', async () => {
    await pc.createIndex({ name: INDEX_NAME, dimension: DIM });
    const result = await pc.listIndexes();
    const names = result.indexes.map((i) => i.name);
    expect(names).toContain(INDEX_NAME);
  });

  it('upsert and fetch vectors', async () => {
    await pc.createIndex({ name: INDEX_NAME, dimension: DIM });
    const idx = pc.index(INDEX_NAME);

    const upsert = await idx.upsert({
      vectors: [
        { id: 'v1', values: [1.0, 0.0, 0.0], metadata: { label: 'a' } },
        { id: 'v2', values: [0.0, 1.0, 0.0], metadata: { label: 'b' } },
      ],
    });
    expect(upsert.upsertedCount).toBe(2);

    const fetched = await idx.fetch({ ids: ['v1'] });
    expect(fetched.vectors['v1']).toBeDefined();
  });

  it('query returns nearest neighbor', async () => {
    await pc.createIndex({ name: INDEX_NAME, dimension: DIM });
    const idx = pc.index(INDEX_NAME);
    await idx.upsert({
      vectors: [
        { id: 'v1', values: [1.0, 0.0, 0.0] },
        { id: 'v2', values: [0.0, 1.0, 0.0] },
      ],
    });

    const result = await idx.query({ vector: [1.0, 0.0, 0.0], topK: 2 });
    expect(result.matches.length).toBeGreaterThanOrEqual(1);
    expect(result.matches[0].id).toBe('v1');
  });

  it('delete by ids', async () => {
    await pc.createIndex({ name: INDEX_NAME, dimension: DIM });
    const idx = pc.index(INDEX_NAME);
    await idx.upsert({ vectors: [{ id: 'v1', values: [1.0, 0.0, 0.0] }] });
    await idx.delete({ ids: ['v1'] });

    const fetched = await idx.fetch({ ids: ['v1'] });
    expect(fetched.vectors['v1']).toBeUndefined();
  });

  it('describeIndexStats returns vector count', async () => {
    await pc.createIndex({ name: INDEX_NAME, dimension: DIM });
    const idx = pc.index(INDEX_NAME);
    await idx.upsert({ vectors: [{ id: 'v1', values: [1.0, 0.0, 0.0] }] });

    const stats = await idx.describeIndexStats();
    expect(stats.dimension).toBe(DIM);
    expect(stats.totalVectorCount).toBeGreaterThanOrEqual(1);
  });

  it('throws OnecortexHttpError for missing index', async () => {
    await expect(pc.describeIndex('nonexistent-index-xyz')).rejects.toThrow(OnecortexHttpError);
  });

  it('hybrid query returns matches', async () => {
    const hybridIdx = 'ts-sdk-hybrid-test';
    try {
      await pc.createIndex({ name: hybridIdx, dimension: DIM, metric: 'cosine', bm25_enabled: true });
      const idx = pc.index(hybridIdx);
      await idx.upsert({
        vectors: [
          { id: 'v1', values: [1.0, 0.0, 0.0], text: 'machine learning basics' },
          { id: 'v2', values: [0.0, 1.0, 0.0], text: 'cooking recipes' },
        ],
      });

      const result = await idx.queryHybrid({
        vector: [1.0, 0.0, 0.0],
        text: 'machine learning',
        topK: 2,
      });
      expect(result.matches.length).toBeGreaterThanOrEqual(1);
    } finally {
      try { await pc.deleteIndex(hybridIdx); } catch {}
    }
  });

  it('query with rerank is accepted', async () => {
    const rerankIdx = 'ts-sdk-rerank-test';
    try {
      await pc.createIndex({ name: rerankIdx, dimension: DIM, metric: 'cosine', bm25_enabled: true });
      const idx = pc.index(rerankIdx);
      await idx.upsert({
        vectors: [
          { id: 'v1', values: [1.0, 0.0, 0.0], metadata: { text: 'machine learning' }, text: 'machine learning' },
          { id: 'v2', values: [0.0, 1.0, 0.0], metadata: { text: 'cooking' }, text: 'cooking' },
        ],
      });

      const result = await idx.query({
        vector: [1.0, 0.0, 0.0],
        topK: 5,
        rerank: { query: 'machine learning', topN: 1, rankField: 'text' },
      });
      expect(result.matches.length).toBeGreaterThanOrEqual(1);
    } finally {
      try { await pc.deleteIndex(rerankIdx); } catch {}
    }
  });

  it('hybrid query with rerank is accepted', async () => {
    const hybridRerankIdx = 'ts-sdk-hybrid-rerank-test';
    try {
      await pc.createIndex({ name: hybridRerankIdx, dimension: DIM, metric: 'cosine', bm25_enabled: true });
      const idx = pc.index(hybridRerankIdx);
      await idx.upsert({
        vectors: [
          { id: 'v1', values: [1.0, 0.0, 0.0], text: 'machine learning basics' },
          { id: 'v2', values: [0.0, 1.0, 0.0], text: 'cooking recipes' },
        ],
      });

      const result = await idx.queryHybrid({
        vector: [1.0, 0.0, 0.0],
        text: 'machine learning',
        topK: 5,
        rerank: { query: 'machine learning', topN: 1 },
      });
      expect(result.matches.length).toBeGreaterThanOrEqual(1);
    } finally {
      try { await pc.deleteIndex(hybridRerankIdx); } catch {}
    }
  });
});
