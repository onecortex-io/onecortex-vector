import type { OnecortexConfig } from './types';

export class OnecortexHttpError extends Error {
  constructor(
    public readonly statusCode: number,
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'OnecortexHttpError';
  }
}

const RETRY_DELAYS = [1000, 2000, 4000];

export class HttpClient {
  private readonly host: string;
  private readonly headers: Record<string, string>;

  constructor(config: OnecortexConfig) {
    this.host = (config.host ?? 'http://localhost:8080').replace(/\/$/, '');
    this.headers = {
      'Api-Key': config.apiKey,
      'Content-Type': 'application/json',
    };
  }

  private async doFetch(method: string, path: string, body?: unknown): Promise<unknown> {
    const url = `${this.host}${path}`;
    const init: RequestInit = {
      method,
      headers: this.headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    };

    for (let attempt = 0; attempt <= RETRY_DELAYS.length; attempt++) {
      if (attempt > 0) {
        await new Promise((r) => setTimeout(r, RETRY_DELAYS[attempt - 1]));
      }

      const res = await fetch(url, init);

      if (res.status === 429 || res.status >= 500) {
        if (attempt < RETRY_DELAYS.length) continue;
      }

      if (!res.ok) {
        let code = 'UNKNOWN';
        let message = res.statusText;
        try {
          const json = (await res.json()) as { error?: { code?: string; message?: string } };
          code = json.error?.code ?? code;
          message = json.error?.message ?? message;
        } catch {}
        throw new OnecortexHttpError(res.status, code, message);
      }

      // 202 Accepted and 204 No Content may have no body
      if (res.status === 202 || res.status === 204) return {};

      const text = await res.text();
      return text ? JSON.parse(text) : {};
    }

    throw new OnecortexHttpError(500, 'INTERNAL', 'Request failed after retries');
  }

  async get(path: string, params?: Record<string, string>): Promise<unknown> {
    const url = params ? `${path}?${new URLSearchParams(params)}` : path;
    return this.doFetch('GET', url);
  }

  async post(path: string, body?: unknown): Promise<unknown> {
    return this.doFetch('POST', path, body);
  }

  async delete(path: string): Promise<unknown> {
    return this.doFetch('DELETE', path);
  }

  async patch(path: string, body?: unknown): Promise<unknown> {
    return this.doFetch('PATCH', path, body);
  }
}
