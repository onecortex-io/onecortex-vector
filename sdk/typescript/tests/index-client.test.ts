import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { Onecortex } from '../src/client';

const BASE = 'http://test-server:8080';
const IDX_NAME = 'test-idx';
const IDX_BASE = `${BASE}/indexes/${IDX_NAME}`;

const QUERY_RESPONSE = {
  matches: [{ id: 'v1', score: 0.99 }],
  namespace: '',
  results: [],
};

const UPSERT_RESPONSE = { upsertedCount: 2 };

const FETCH_RESPONSE = {
  vectors: { v1: { id: 'v1', values: [1.0, 0.0, 0.0], metadata: {} } },
  namespace: '',
};

const LIST_RESPONSE = {
  vectors: [{ id: 'v1' }, { id: 'v2' }],
  namespace: '',
};

const STATS_RESPONSE = {
  namespaces: { '': { vectorCount: 2 } },
  dimension: 3,
  indexFullness: 0.001,
  totalVectorCount: 2,
};

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterAll(() => server.close());

function makeIndex() {
  const pc = new Onecortex({ apiKey: 'key123', host: BASE });
  return pc.index(IDX_NAME);
}

describe('Index client', () => {
  it('upsert returns upsertedCount', async () => {
    server.use(
      http.post(`${IDX_BASE}/vectors/upsert`, () => HttpResponse.json(UPSERT_RESPONSE)),
    );
    const idx = makeIndex();
    const result = await idx.upsert({
      vectors: [
        { id: 'v1', values: [1.0, 0.0, 0.0] },
        { id: 'v2', values: [0.0, 1.0, 0.0] },
      ],
    });
    expect(result.upsertedCount).toBe(2);
  });

  it('query returns matches', async () => {
    server.use(
      http.post(`${IDX_BASE}/query`, () => HttpResponse.json(QUERY_RESPONSE)),
    );
    const idx = makeIndex();
    const result = await idx.query({ vector: [1.0, 0.0, 0.0], topK: 1 });
    expect(result.matches[0].id).toBe('v1');
    expect(result.matches[0].score).toBe(0.99);
  });

  it('queryHybrid returns matches', async () => {
    server.use(
      http.post(`${IDX_BASE}/query/hybrid`, () => HttpResponse.json(QUERY_RESPONSE)),
    );
    const idx = makeIndex();
    const result = await idx.queryHybrid({
      vector: [1.0, 0.0, 0.0],
      queryText: 'hello',
      topK: 5,
    });
    expect(result.matches[0].id).toBe('v1');
  });

  it('fetch returns vectors map', async () => {
    server.use(
      http.post(`${IDX_BASE}/vectors/fetch`, () => HttpResponse.json(FETCH_RESPONSE)),
    );
    const idx = makeIndex();
    const result = await idx.fetch({ ids: ['v1'] });
    expect(result.vectors['v1']).toBeDefined();
  });

  it('delete resolves without error', async () => {
    server.use(
      http.post(`${IDX_BASE}/vectors/delete`, () => HttpResponse.json({})),
    );
    const idx = makeIndex();
    await expect(idx.delete({ ids: ['v1'] })).resolves.toBeUndefined();
  });

  it('update resolves without error', async () => {
    server.use(
      http.post(`${IDX_BASE}/vectors/update`, () => HttpResponse.json({})),
    );
    const idx = makeIndex();
    await expect(idx.update({ id: 'v1', setMetadata: { x: 1 } })).resolves.toBeUndefined();
  });

  it('list returns vectors array', async () => {
    server.use(
      http.get(`${IDX_BASE}/vectors/list`, () => HttpResponse.json(LIST_RESPONSE)),
    );
    const idx = makeIndex();
    const result = await idx.list();
    expect(result.vectors).toHaveLength(2);
  });

  it('describeIndexStats returns stats', async () => {
    server.use(
      http.post(`${IDX_BASE}/describe_index_stats`, () => HttpResponse.json(STATS_RESPONSE)),
    );
    const idx = makeIndex();
    const stats = await idx.describeIndexStats();
    expect(stats.totalVectorCount).toBe(2);
    expect(stats.dimension).toBe(3);
  });
});
