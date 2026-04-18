from dataclasses import dataclass
from typing import Optional, Dict, List
import time


@dataclass
class CacheEntry:
    key: str
    value: str
    expires_at: float
    access_count: int


class LRUCache:
    def __init__(self, max_size: int = 1000, ttl_seconds: float = 300.0):
        self.max_size = max_size
        self.ttl_seconds = ttl_seconds
        self._store: Dict[str, CacheEntry] = {}

    def get(self, key: str) -> Optional[str]:
        entry = self._store.get(key)
        if entry is None:
            return None
        if time.time() > entry.expires_at:
            del self._store[key]
            return None
        entry.access_count += 1
        return entry.value

    def put(self, key: str, value: str) -> None:
        if len(self._store) >= self.max_size:
            self._evict_oldest()
        self._store[key] = CacheEntry(
            key=key,
            value=value,
            expires_at=time.time() + self.ttl_seconds,
            access_count=0,
        )

    def _evict_oldest(self) -> None:
        if not self._store:
            return
        oldest_key = min(self._store, key=lambda k: self._store[k].access_count)
        del self._store[oldest_key]

    def invalidate(self, key: str) -> bool:
        if key in self._store:
            del self._store[key]
            return True
        return False

    def clear_expired_entries(self) -> int:
        now = time.time()
        expired = [k for k, v in self._store.items() if now > v.expires_at]
        for k in expired:
            del self._store[k]
        return len(expired)

    def compute_cache_hit_rate(self) -> float:
        total = sum(e.access_count for e in self._store.values())
        if not self._store:
            return 0.0
        return total / len(self._store)


def extract_path_components(path: str) -> List[str]:
    parts = [p for p in path.split("/") if p]
    return parts


def normalize_file_path(path: str) -> str:
    components = extract_path_components(path)
    result: List[str] = []
    for comp in components:
        if comp == "..":
            if result:
                result.pop()
        elif comp != ".":
            result.append(comp)
    return "/" + "/".join(result)
