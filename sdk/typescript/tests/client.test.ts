import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import { Onecortex } from '../src/client';
import { OnecortexHttpError } from '../src/http';

const BASE = 'http://test-server:8080';

const INDEX_RESPONSE = {
  name: 'test-idx',
  dimension: 3,
  metric: 'cosine',
  status: { ready: true, state: 'Ready' },
  host: 'test-server:8080',
  spec: {},
  vector_type: 'dense',
};

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterAll(() => server.close());

function makeClient() {
  return new Onecortex({ apiKey: 'key123', host: BASE });
}

describe('Onecortex client', () => {
  it('createIndex returns IndexDescription', async () => {
    server.use(
      http.post(`${BASE}/indexes`, () => HttpResponse.json(INDEX_RESPONSE)),
    );
    const pc = makeClient();
    const idx = await pc.createIndex({ name: 'test-idx', dimension: 3, metric: 'cosine' });
    expect(idx.name).toBe('test-idx');
    expect(idx.dimension).toBe(3);
  });

  it('createIndex accepts and ignores spec', async () => {
    server.use(
      http.post(`${BASE}/indexes`, () => HttpResponse.json(INDEX_RESPONSE)),
    );
    const pc = makeClient();
    const idx = await pc.createIndex({
      name: 'test-idx',
      dimension: 3,
      spec: { serverless: { cloud: 'aws', region: 'us-east-1' } },
    });
    expect(idx.name).toBe('test-idx');
  });

  it('describeIndex returns IndexDescription', async () => {
    server.use(
      http.get(`${BASE}/indexes/test-idx`, () => HttpResponse.json(INDEX_RESPONSE)),
    );
    const pc = makeClient();
    const idx = await pc.describeIndex('test-idx');
    expect(idx.metric).toBe('cosine');
  });

  it('listIndexes returns array', async () => {
    server.use(
      http.get(`${BASE}/indexes`, () => HttpResponse.json({ indexes: [INDEX_RESPONSE] })),
    );
    const pc = makeClient();
    const result = await pc.listIndexes();
    expect(result.indexes).toHaveLength(1);
    expect(result.indexes[0].name).toBe('test-idx');
  });

  it('deleteIndex resolves without error', async () => {
    server.use(
      http.delete(`${BASE}/indexes/test-idx`, () => new HttpResponse(null, { status: 202 })),
    );
    const pc = makeClient();
    await expect(pc.deleteIndex('test-idx')).resolves.toBeUndefined();
  });

  it('configureIndex sends patch', async () => {
    server.use(
      http.patch(`${BASE}/indexes/test-idx`, () => HttpResponse.json(INDEX_RESPONSE)),
    );
    const pc = makeClient();
    const result = await pc.configureIndex('test-idx', { tags: { env: 'prod' } });
    expect(result.name).toBe('test-idx');
  });

  it('throws OnecortexHttpError on 404', async () => {
    server.use(
      http.get(`${BASE}/indexes/missing`, () =>
        HttpResponse.json(
          { error: { code: 'NOT_FOUND', message: 'not found' } },
          { status: 404 },
        ),
      ),
    );
    const pc = makeClient();
    await expect(pc.describeIndex('missing')).rejects.toThrow(OnecortexHttpError);
  });

  it('index() returns an Index instance', () => {
    const pc = makeClient();
    const idx = pc.index('my-index');
    expect(idx).toBeDefined();
  });
});
