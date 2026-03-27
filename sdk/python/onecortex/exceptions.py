class OnecortexError(Exception):
    """Base exception for all Onecortex SDK errors."""
    def __init__(self, message: str, status_code: int | None = None):
        super().__init__(message)
        self.status_code = status_code

class NotFoundError(OnecortexError):
    """Raised when a resource (index, vector) does not exist."""

class AlreadyExistsError(OnecortexError):
    """Raised when trying to create a resource that already exists."""

class InvalidArgumentError(OnecortexError):
    """Raised for invalid request parameters."""

class UnauthorizedError(OnecortexError):
    """Raised when the API key is missing or invalid."""

class PermissionDeniedError(OnecortexError):
    """Raised when the key lacks access to the requested namespace."""

class OnecortexServerError(OnecortexError):
    """Raised for unexpected server errors (5xx)."""
