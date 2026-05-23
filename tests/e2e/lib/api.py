import json
import urllib.error
import urllib.request


class DexterApiError(RuntimeError):
    pass


class DexterApi:
    def __init__(self, base_url="http://127.0.0.1:9877"):
        self.base_url = base_url.rstrip("/")

    def get(self, path):
        return self._request("GET", path)

    def post(self, path, body=None):
        return self._request("POST", path, body or {})

    def _request(self, method, path, body=None):
        data = None
        headers = {}
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            headers["content-type"] = "application/json"

        request = urllib.request.Request(
            f"{self.base_url}{path}",
            data=data,
            headers=headers,
            method=method,
        )

        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                payload = response.read().decode("utf-8")
        except urllib.error.HTTPError as error:
            payload = error.read().decode("utf-8")
            raise DexterApiError(f"{method} {path} failed: {error.code} {payload}") from error
        except urllib.error.URLError as error:
            raise DexterApiError(f"{method} {path} failed: {error}") from error

        return json.loads(payload) if payload else {}
