#!/usr/bin/env python3
"""Run a GraphIQ vs real ripgrep retrieval benchmark.

The benchmark is deliberately kept outside the Rust benchmark crate because
ripgrep returns matching lines/files while GraphIQ returns symbols.  This
adapter makes that unit mismatch explicit and deterministic:

* GraphIQ results are parsed from `graphiq search` and represented as
  `relative/path::symbol` items.
* ripgrep is invoked on the same source snapshot.  Natural-language questions
  are converted to an OR query over meaningful tokens, and matching lines are
  mapped to the innermost indexed symbol.  Candidates are ranked by distinct
  query-token coverage, then matching-line count, then first occurrence.
* file-path questions use `rg --files` and are evaluated at file level.

This is a relevance benchmark, not a latency benchmark.  The database must
already have been indexed before this script is run.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import shutil
import sqlite3
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


SEARCHABLE_KINDS = {
    "function",
    "method",
    "class",
    "interface",
    "type_alias",
    "struct",
    "enum",
    "trait",
    "constant",
    "export",
}

# These are query-language words rather than useful search terms.  We retain
# domain words such as "file", "relay", and "transfer" because those are
# exactly the terms a line-oriented baseline should be allowed to search.
STOP_WORDS = {
    "a",
    "an",
    "all",
    "and",
    "are",
    "as",
    "at",
    "before",
    "between",
    "by",
    "can",
    "code",
    "does",
    "each",
    "every",
    "for",
    "from",
    "how",
    "in",
    "into",
    "is",
    "it",
    "list",
    "of",
    "on",
    "or",
    "that",
    "the",
    "their",
    "then",
    "this",
    "to",
    "under",
    "what",
    "when",
    "where",
    "which",
    "who",
    "with",
    "without",
}

GRAPH_LINE_RE = re.compile(
    r"^#\d+\s+[-+0-9.eE]+\s+(.+):(\d+)\s+[^\s:]+::(.*)$"
)
TOKEN_RE = re.compile(r"[A-Za-z][A-Za-z0-9_]*")


def usable_symbol_name(name: str) -> bool:
    # Some parsers represent a large object literal or stylesheet block as a
    # single constant/module symbol. Such names are not useful ranked items
    # and can make machine-readable benchmark output enormous.
    return bool(name.strip()) and "\n" not in name and len(name) <= 160


@dataclass(frozen=True)
class Symbol:
    key: str
    path: str
    name: str
    kind: str
    start: int
    end: int


@dataclass
class Candidate:
    token_hits: set[str]
    line_hits: int
    first_order: int


def load_symbols(db_path: Path) -> tuple[dict[str, Symbol], dict[str, list[Symbol]]]:
    conn = sqlite3.connect(db_path)
    try:
        rows = conn.execute(
            """
            SELECT s.name, s.kind, f.path, s.line_start, s.line_end
            FROM symbols AS s
            JOIN files AS f ON f.id = s.file_id
            WHERE s.kind IN ({})
            ORDER BY f.path, s.line_start, s.line_end, s.id
            """.format(",".join("?" for _ in SEARCHABLE_KINDS)),
            tuple(sorted(SEARCHABLE_KINDS)),
        )
        by_key: dict[str, Symbol] = {}
        by_path: dict[str, list[Symbol]] = defaultdict(list)
        for name, kind, path, start, end in rows:
            symbol = Symbol(
                key=f"{path}::{name}",
                path=path,
                name=name,
                kind=kind,
                start=int(start),
                end=int(end),
            )
            by_key.setdefault(symbol.key, symbol)
            by_path[path].append(symbol)
        return by_key, dict(by_path)
    finally:
        conn.close()


def meaningful_tokens(query: str) -> list[str]:
    raw = [m.group(0).lower() for m in TOKEN_RE.finditer(query)]
    tokens: list[str] = []
    for token in raw:
        if len(token) < 2 or token in STOP_WORDS:
            continue
        if token not in tokens:
            tokens.append(token)
    # A code-shaped one-token query (e.g. `ValidateEntries`) should remain a
    # single exact-ish literal rather than being broken into generic pieces.
    if not tokens:
        tokens = [query.lower().strip()]
    return tokens


def resolve_symbol(
    path: str, line_zero_based: int, by_path: dict[str, list[Symbol]]
) -> Symbol | None:
    symbols = by_path.get(path, [])
    containing = [
        s for s in symbols if s.start <= line_zero_based <= s.end
    ]
    if containing:
        # An indexed method/function is more useful than its containing class;
        # the shortest span is the innermost symbol in normal source layouts.
        return min(containing, key=lambda s: (s.end - s.start, s.start, s.key))

    # Documentation immediately above a declaration is part of the searchable
    # symbol context for GraphIQ.  Give rg the same practical mapping for a
    # one-line gap, but never assign arbitrary import/module lines.
    following = [s for s in symbols if 0 < s.start - line_zero_based <= 2]
    if following:
        return min(following, key=lambda s: (s.start - line_zero_based, s.start, s.key))
    return None


def run_rg_json(
    rg: str, repo: Path, tokens: list[str]
) -> list[tuple[str, int, str]]:
    cmd = [
        rg,
        "--json",
        "--ignore-case",
        "--hidden",
        "--no-ignore-vcs",
        "--glob",
        "!**/.git/**",
        "--sort",
        "path",
        "--fixed-strings",
    ]
    for token in tokens:
        cmd.extend(["-e", token])
    cmd.append(str(repo))
    proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if proc.returncode not in (0, 1):
        raise RuntimeError(
            f"ripgrep failed ({proc.returncode}) for {tokens!r}: {proc.stderr.strip()}"
        )

    matches: list[tuple[str, int, str]] = []
    for line in proc.stdout.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") != "match":
            continue
        data = event.get("data", {})
        path = data.get("path", {}).get("text")
        line_number = data.get("line_number")
        text = data.get("lines", {}).get("text", "")
        if isinstance(path, str) and isinstance(line_number, int):
            # rg emits paths relative to cwd when cwd is the repository.  The
            # command above receives an absolute repo path, so normalize here.
            rel = Path(path)
            try:
                rel = rel.relative_to(repo)
            except ValueError:
                pass
            matches.append((rel.as_posix(), line_number, text))
    return matches


def rg_symbol_results(
    rg: str,
    repo: Path,
    query: str,
    by_path: dict[str, list[Symbol]],
) -> list[str]:
    tokens = meaningful_tokens(query)
    candidates: dict[str, Candidate] = {}
    order = 0
    for path, line_number, text in run_rg_json(rg, repo, tokens):
        symbol = resolve_symbol(path, line_number - 1, by_path)
        if symbol is None or not usable_symbol_name(symbol.name):
            continue
        lower = text.lower()
        hits = {token for token in tokens if token in lower}
        if not hits:
            # A defensive fallback for unusual unicode/case-folding behavior.
            hits = {tokens[0]}
        candidate = candidates.get(symbol.key)
        if candidate is None:
            candidate = Candidate(set(), 0, order)
            candidates[symbol.key] = candidate
            order += 1
        candidate.token_hits.update(hits)
        candidate.line_hits += 1

    ranked = sorted(
        candidates.items(),
        key=lambda item: (
            -len(item[1].token_hits),
            -item[1].line_hits,
            item[1].first_order,
            item[0],
        ),
    )
    return [key for key, _ in ranked[:10]]


def rg_file_results(rg: str, repo: Path, query: str) -> list[str]:
    cmd = [
        rg,
        "--files",
        "--hidden",
        "--no-ignore-vcs",
        "--glob",
        "!**/.git/**",
        "--sort",
        "path",
        str(repo),
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if proc.returncode not in (0, 1):
        raise RuntimeError(f"ripgrep --files failed: {proc.stderr.strip()}")
    path_matches = re.findall(r"(?:[A-Za-z0-9_.-]+/)+[A-Za-z0-9_.-]+", query)
    needle = (path_matches[0] if path_matches else query).replace("\\", "/").lower()
    results: list[str] = []
    for line in proc.stdout.splitlines():
        path = Path(line).as_posix()
        try:
            path = Path(path).relative_to(repo).as_posix()
        except ValueError:
            pass
        if needle in path.lower():
            results.append(f"file:{path}")
    return results[:10]


def parse_graphiq_results(
    output: str, symbols: dict[str, Symbol], searchable_only: bool = True
) -> list[str]:
    results: list[str] = []
    for line in output.splitlines():
        match = GRAPH_LINE_RE.match(line)
        if match is None:
            continue
        path, _line, name = match.groups()
        if searchable_only and not usable_symbol_name(name):
            continue
        key = f"{path}::{name}"
        # Compare the same indexed, symbol-level unit as the rg adapter;
        # parser-only imports/sections/fields are not retrieval targets.  File
        # path queries deliberately keep those rows because only their path
        # is projected into the common file-level unit later.
        if searchable_only and key not in symbols:
            continue
        if key not in results:
            results.append(key)
        if len(results) == 10:
            break
    return results


def run_graphiq(
    graphiq: str,
    db: Path,
    query: str,
    symbols: dict[str, Symbol],
    searchable_only: bool = True,
    executable_evidence: bool = False,
) -> list[str]:
    command = [graphiq, "search", "--db", str(db), "--top", "10"]
    if executable_evidence:
        command.append("--executable-evidence")
    command.append(query)
    proc = subprocess.run(
        command,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"GraphIQ failed ({proc.returncode}) for {query!r}: {proc.stderr.strip()}"
        )
    return parse_graphiq_results(proc.stdout, symbols, searchable_only)


def dcg(values: Iterable[float], k: int = 10) -> float:
    return sum(value / math.log2(rank + 2) for rank, value in enumerate(list(values)[:k]))


def query_relevance(q: dict[str, Any], key: str) -> int:
    if "relevance_files" in q:
        if not key.startswith("file:"):
            return 0
        return int(q["relevance_files"].get(key.removeprefix("file:"), 0))
    return int(q.get("relevance", {}).get(key, 0))


def ideal_relevances(q: dict[str, Any]) -> list[float]:
    if "relevance_files" in q:
        values = q["relevance_files"].values()
    else:
        values = q.get("relevance", {}).values()
    return sorted((float(v) for v in values), reverse=True)


def is_relevant_result(key: str, q: dict[str, Any], threshold: int) -> bool:
    if "expected_symbol" in q:
        if q["category"] == "file-path":
            return key == f"file:{q['expected_file']}"
        return key == f"{q['expected_file']}::{q['expected_symbol']}"
    return query_relevance(q, key) >= threshold


def h_at(results: list[str], q: dict[str, Any], k: int, threshold: int) -> float:
    return float(any(is_relevant_result(key, q, threshold) for key in results[:k]))


def score_ndcg(results: list[str], q: dict[str, Any]) -> float:
    actual = [float(query_relevance(q, key)) for key in results]
    ideal = ideal_relevances(q)
    denominator = dcg(ideal, 10)
    return dcg(actual, 10) / denominator if denominator else 0.0


def score_mrr(results: list[str], q: dict[str, Any]) -> float:
    if q["category"] == "file-path":
        expected = f"file:{q['expected_file']}"
    else:
        expected = f"{q['expected_file']}::{q['expected_symbol']}"
    try:
        rank = results.index(expected)
    except ValueError:
        return 0.0
    return 1.0 / (rank + 1)


def load_queries(path: Path) -> list[dict[str, Any]]:
    queries = json.loads(path.read_text())
    if not isinstance(queries, list):
        raise ValueError(f"{path} must contain a JSON array")
    return queries


def validate_queries(
    queries: list[dict[str, Any]],
    symbols: dict[str, Symbol],
    by_path: dict[str, list[Symbol]],
    metric: str,
    expected_count: int = 50,
) -> None:
    categories = Counter(q.get("category", "") for q in queries)
    if len(queries) != expected_count:
        raise ValueError(f"expected {expected_count} queries, found {len(queries)}")
    if expected_count % 10 != 0:
        raise ValueError("expected query count must be divisible by 10 categories")
    expected_per_category = expected_count // 10
    if (
        sorted(categories.values()) != [expected_per_category] * len(categories)
        or len(categories) != 10
    ):
        raise ValueError(
            f"expected 10 categories with {expected_per_category} queries each: {categories}"
        )
    for q in queries:
        if metric == "mrr":
            key = f"{q['expected_file']}::{q['expected_symbol']}"
            if key not in symbols:
                raise ValueError(f"MRR target is not indexed: {key}")
        else:
            if not q.get("relevance") and not q.get("relevance_files"):
                raise ValueError(f"NDCG query has no relevance judgments: {q['id']}")
            for key in q.get("relevance", {}):
                if key not in symbols:
                    raise ValueError(f"NDCG target is not indexed: {key}")
            for path in q.get("relevance_files", {}):
                if path not in by_path:
                    raise ValueError(f"NDCG file target is not indexed: {path}")


def format_scores(
    values: dict[str, dict[str, float]], metric: str, methods: tuple[str, ...]
) -> str:
    header = "MRR@10" if metric == "mrr" else "NDCG@10"
    header += " | H@1 | H@3 | H@5 | H@10"
    lines = [header, ""]
    for method in methods:
        row = values[method]
        lines.append(
            f"{method} | {row['metric']:.3f} | {row['h1']:.3f} | "
            f"{row['h3']:.3f} | {row['h5']:.3f} | {row['h10']:.3f}"
        )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", default="benchmark", help="human-readable benchmark name")
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--db", type=Path, required=True)
    parser.add_argument("--queries", type=Path, required=True)
    parser.add_argument("--metric", choices=("mrr", "ndcg"), required=True)
    parser.add_argument(
        "--expected-count",
        type=int,
        default=50,
        help="number of queries and ten-category balance (default: 50)",
    )
    parser.add_argument(
        "--graph-only",
        action="store_true",
        help="evaluate GraphIQ only; do not invoke or report a ripgrep baseline",
    )
    parser.add_argument(
        "--graphiq",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "target/release/graphiq",
    )
    parser.add_argument("--rg", default=shutil.which("rg") or "rg")
    parser.add_argument("--raw-output", type=Path)
    parser.add_argument(
        "--executable-evidence",
        action="store_true",
        help="include opt-in test-to-production evidence edges in GraphWalk",
    )
    args = parser.parse_args()

    repo = args.repo.resolve()
    db = args.db.resolve()
    queries = load_queries(args.queries.resolve())
    symbols, by_path = load_symbols(db)
    validate_queries(
        queries, symbols, by_path, args.metric, expected_count=args.expected_count
    )
    methods = ("GraphIQ",) if args.graph_only else ("GraphIQ", "ripgrep")

    print(
        f"Benchmark: {args.name} {args.metric.upper()} ({len(queries)} queries)",
        file=sys.stderr,
    )
    print(f"Repository: {repo}", file=sys.stderr)
    print(f"Database: {db}", file=sys.stderr)
    if not args.graph_only:
        print(f"ripgrep: {args.rg}", file=sys.stderr)

    threshold = 3 if args.metric == "mrr" else 2
    records: list[dict[str, Any]] = []
    for index, q in enumerate(queries, start=1):
        if q["category"] == "file-path":
            if not args.graph_only:
                rg_results = rg_file_results(args.rg, repo, q["query"])
            # GraphIQ still returns symbols; project them to file items so the
            # file-path category compares the same retrieval unit.
            graph_symbols = run_graphiq(
                str(args.graphiq),
                db,
                q["query"],
                symbols,
                searchable_only=False,
                executable_evidence=args.executable_evidence,
            )
            graph_results = []
            for key in graph_symbols:
                path = key.split("::", 1)[0]
                file_key = f"file:{path}"
                if file_key not in graph_results:
                    graph_results.append(file_key)
            graph_results = graph_results[:10]
        else:
            graph_results = run_graphiq(
                str(args.graphiq),
                db,
                q["query"],
                symbols,
                executable_evidence=args.executable_evidence,
            )
            if not args.graph_only:
                rg_results = rg_symbol_results(args.rg, repo, q["query"], by_path)

        if args.metric == "mrr":
            graph_metric = score_mrr(graph_results, q)
            rg_metric = None if args.graph_only else score_mrr(rg_results, q)
        else:
            graph_metric = score_ndcg(graph_results, q)
            rg_metric = None if args.graph_only else score_ndcg(rg_results, q)

        record: dict[str, Any] = {
            "id": q["id"],
            "category": q["category"],
            "query": q["query"],
            "GraphIQ": {
                "results": graph_results,
                "metric": graph_metric,
                "h1": h_at(graph_results, q, 1, threshold),
                "h3": h_at(graph_results, q, 3, threshold),
                "h5": h_at(graph_results, q, 5, threshold),
                "h10": h_at(graph_results, q, 10, threshold),
            },
        }
        if not args.graph_only:
            record["ripgrep"] = {
                "results": rg_results,
                "metric": rg_metric,
                "h1": h_at(rg_results, q, 1, threshold),
                "h3": h_at(rg_results, q, 3, threshold),
                "h5": h_at(rg_results, q, 5, threshold),
                "h10": h_at(rg_results, q, 10, threshold),
            }
        records.append(record)
        print(f"[{index:02d}/{len(queries)}] {q['id']}", file=sys.stderr)

    overall: dict[str, dict[str, float]] = {}
    for method in methods:
        overall[method] = {
            "metric": sum(r[method]["metric"] for r in records) / len(records),
            "h1": sum(r[method]["h1"] for r in records) / len(records),
            "h3": sum(r[method]["h3"] for r in records) / len(records),
            "h5": sum(r[method]["h5"] for r in records) / len(records),
            "h10": sum(r[method]["h10"] for r in records) / len(records),
        }

    by_category: dict[str, dict[str, dict[str, float]]] = {}
    for category in sorted({r["category"] for r in records}):
        subset = [r for r in records if r["category"] == category]
        by_category[category] = {}
        for method in methods:
            by_category[category][method] = {
                "metric": sum(r[method]["metric"] for r in subset) / len(subset),
                "h1": sum(r[method]["h1"] for r in subset) / len(subset),
                "h3": sum(r[method]["h3"] for r in subset) / len(subset),
                "h5": sum(r[method]["h5"] for r in subset) / len(subset),
                "h10": sum(r[method]["h10"] for r in subset) / len(subset),
            }

    print(format_scores(overall, args.metric, methods))
    print("\nCategory | Method | " + ("MRR@10" if args.metric == "mrr" else "NDCG@10") + " | H@1 | H@3 | H@5 | H@10")
    for category in sorted(by_category):
        for method in methods:
            row = by_category[category][method]
            print(
                f"{category} | {method} | {row['metric']:.3f} | {row['h1']:.3f} | "
                f"{row['h3']:.3f} | {row['h5']:.3f} | {row['h10']:.3f}"
            )

    if args.raw_output:
        args.raw_output.parent.mkdir(parents=True, exist_ok=True)
        args.raw_output.write_text(
            json.dumps(
                {
                    "metric": args.metric,
                    "name": args.name,
                    "repo": str(repo),
                    "db": str(db),
                    "queries": str(args.queries.resolve()),
                    "records": records,
                    "overall": overall,
                    "by_category": by_category,
                },
                indent=2,
            )
            + "\n"
        )
        print(f"Raw results: {args.raw_output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
