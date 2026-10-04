"""Mini Python service with decorators, classes, and imports."""

from dataclasses import dataclass
from typing import Optional, List
import json
import os

@dataclass
class Config:
    host: str = "localhost"
    port: int = 8080
    debug: bool = False

class BaseHandler:
    """Base class for request handlers."""
    def __init__(self, config: Config):
        self.config = config

    def handle(self, request: dict) -> dict:
        raise NotImplementedError

class AuthHandler(BaseHandler):
    """Authentication handler with decorator-based routing."""
    def handle(self, request: dict) -> dict:
        token = request.get("token", "")
        if not token:
            return {"status": 401, "error": "unauthorized"}
        return {"status": 200, "user": "admin"}

class DataHandler(BaseHandler):
    """Data CRUD handler."""
    def __init__(self, config: Config, db_path: str):
        super().__init__(config)
        self.db_path = db_path

    def handle(self, request: dict) -> dict:
        method = request.get("method", "GET")
        if method == "GET":
            return self._read(request)
        elif method == "POST":
            return self._write(request)
        return {"status": 405}

    def _read(self, request: dict) -> dict:
        key = request.get("key", "")
        return {"status": 200, "data": f"value_for_{key}"}

    def _write(self, request: dict) -> dict:
        return {"status": 201, "written": True}

def load_config(path: Optional[str] = None) -> Config:
    """Load configuration from file or environment."""
    if path and os.path.exists(path):
        with open(path) as f:
            data = json.load(f)
        return Config(**data)
    return Config(
        host=os.getenv("HOST", "0.0.0.0"),
        port=int(os.getenv("PORT", "9000")),
    )

def create_app(config: Config) -> List[BaseHandler]:
    """Application factory."""
    handlers: List[BaseHandler] = [
        AuthHandler(config),
        DataHandler(config, "/tmp/data.db"),
    ]
    return handlers

if __name__ == "__main__":
    cfg = load_config()
    app = create_app(cfg)
    for h in app:
        result = h.handle({"token": "test123", "method": "GET", "key": "x"})
        print(f"{h.__class__.__name__}: {result}")
