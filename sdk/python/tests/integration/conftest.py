import os
import pytest
from onecortex import Onecortex

HOST = os.environ.get("ONECORTEX_HOST", "http://localhost:8080")
API_KEY = os.environ.get("ONECORTEX_API_KEY", "")


@pytest.fixture
def oc_client():
    return Onecortex(api_key=API_KEY, host=HOST)
