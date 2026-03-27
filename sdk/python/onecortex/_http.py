import time
import httpx
from .exceptions import (
    OnecortexError, NotFoundError, AlreadyExistsError,
    InvalidArgumentError, UnauthorizedError, PermissionDeniedError, OnecortexServerError
)

_ERROR_MAP = {
    "NOT_FOUND": NotFoundError,
    "ALREADY_EXISTS": AlreadyExistsError,
    "INVALID_ARGUMENT": InvalidArgumentError,
    "UNAUTHENTICATED": UnauthorizedError,
    "PERMISSION_DENIED": PermissionDeniedError,
}

def _raise_for_response(response: httpx.Response) -> None:
    if response.status_code < 400:
        return
    try:
        body = response.json()
        code = body.get("error", {}).get("code", "UNKNOWN")
        message = body.get("error", {}).get("message", response.text)
    except Exception:
        code = "UNKNOWN"
        message = response.text

    exc_class = _ERROR_MAP.get(code, OnecortexServerError)
    raise exc_class(message, status_code=response.status_code)


class HttpClient:
    """Synchronous httpx client with retry logic and auth header injection."""

    def __init__(self, api_key: str, host: str):
        self._host = host.rstrip("/")
        self._headers = {
            "Api-Key": api_key,
            "Content-Type": "application/json",
        }
        self._client = httpx.Client(headers=self._headers, timeout=30.0)

    def request(self, method: str, path: str, **kwargs) -> httpx.Response:
        url = f"{self._host}{path}"
        # Retry on 429 and 5xx with exponential backoff: 1s, 2s, 4s
        delays = [1, 2, 4]
        last_exc = None
        for attempt, delay in enumerate([0] + delays):
            if delay:
                time.sleep(delay)
            try:
                response = self._client.request(method, url, **kwargs)
                if response.status_code in (429,) or response.status_code >= 500:
                    if attempt < len(delays):
                        last_exc = response
                        continue
                _raise_for_response(response)
                return response
            except (httpx.ConnectError, httpx.TimeoutException) as e:
                last_exc = e
                continue
        if isinstance(last_exc, httpx.Response):
            _raise_for_response(last_exc)
        raise OnecortexServerError(f"Request failed after retries: {last_exc}")

    def get(self, path: str, **kwargs) -> httpx.Response:
        return self.request("GET", path, **kwargs)

    def post(self, path: str, json: dict | None = None, **kwargs) -> httpx.Response:
        return self.request("POST", path, json=json, **kwargs)

    def delete(self, path: str, **kwargs) -> httpx.Response:
        return self.request("DELETE", path, **kwargs)

    def patch(self, path: str, json: dict | None = None, **kwargs) -> httpx.Response:
        return self.request("PATCH", path, json=json, **kwargs)
