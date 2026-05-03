from __future__ import annotations

import argparse
import json
import math
import os
import re
import subprocess
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import UTC, datetime
from functools import lru_cache
from pathlib import Path
from statistics import mean

from scope_spike.tokenizer import count_tokens, tokenizer_metadata

DEFAULT_MANIFEST = Path("scope_spike/study_manifest.json")
DEFAULT_GROUND_TRUTH_ROOT = Path("scope_spike/results/study-20260415-rerun")
DEFAULT_RESULTS_ROOT = Path("scope_spike_v2/results")

TEXT_SUFFIXES = {
    ".c",
    ".cc",
    ".cfg",
    ".clj",
    ".cpp",
    ".cs",
    ".css",
    ".go",
    ".h",
    ".hpp",
    ".html",
    ".java",
    ".js",
    ".json",
    ".jsx",
    ".kt",
    ".kts",
    ".lock",
    ".lua",
    ".md",
    ".py",
    ".rb",
    ".rs",
    ".scss",
    ".sh",
    ".sql",
    ".toml",
    ".ts",
    ".tsx",
    ".txt",
    ".yaml",
    ".yml",
}

STOPWORDS = {
    "a",
    "accepted",
    "across",
    "add",
    "all",
    "an",
    "and",
    "any",
    "are",
    "as",
    "at",
    "behavior",
    "both",
    "bugfix",
    "by",
    "case",
    "change",
    "changes",
    "cli",
    "crate",
    "derive",
    "docs",
    "existing",
    "feature",
    "fields",
    "finishing",
    "focused",
    "for",
    "from",
    "function",
    "helper",
    "in",
    "into",
    "it",
    "its",
    "keep",
    "like",
    "make",
    "new",
    "of",
    "or",
    "parser",
    "preserving",
    "public",
    "refactor",
    "relevant",
    "rename",
    "rule",
    "so",
    "style",
    "support",
    "tests",
    "that",
    "the",
    "their",
    "then",
    "through",
    "to",
    "update",
    "verify",
    "while",
    "with",
    "without",
    "work",
}

BACKTICK_RE = re.compile(r"`([^`]+)`")
QUOTED_RE = re.compile(r'"([^"]+)"')
WORD_RE = re.compile(r"[A-Za-z][A-Za-z0-9_:-]*")
CAMEL_RE = re.compile(r"[A-Z]+(?=[A-Z][a-z]|[0-9]|$)|[A-Z]?[a-z]+|[0-9]+")
IMPORT_RE = re.compile(r"^\s*(?:pub\s+)?(?:use|mod)\s+([^;]+);", re.MULTILINE)
ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:fn|struct|enum|trait|type|mod)\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)


def _split_identifier(raw: str) -> list[str]:
    cleaned = raw.replace("::", " ").replace("/", " ").replace("-", " ").replace(".", " ")
    pieces = []
    for token in cleaned.split():
        pieces.extend(part.lower() for part in CAMEL_RE.findall(token.replace("_", " ")))
    return [piece for piece in pieces if piece]


def _query_terms(text: str) -> tuple[list[str], list[str]]:
    exact_terms = []
    for matcher in (BACKTICK_RE, QUOTED_RE):
        for match in matcher.findall(text):
            if match and len(match) > 1:
                exact_terms.append(match.strip())
    words: list[str] = []
    for raw in WORD_RE.findall(text):
        for token in _split_identifier(raw):
            if len(token) > 1 and token not in STOPWORDS:
                words.append(token)
    return sorted(dict.fromkeys(words)), sorted(dict.fromkeys(exact_terms))


def _normalize_repo_path(path: str) -> str:
    return path.replace("\\", "/").lstrip("./")


def _iter_files(repo_root: Path) -> list[Path]:
    try:
        completed = subprocess.run(
            ["git", "-C", str(repo_root), "ls-files"],
            capture_output=True,
            text=True,
            check=True,
        )
        paths = [_normalize_repo_path(line) for line in completed.stdout.splitlines() if line.strip()]
        return [repo_root / path for path in paths]
    except Exception:
        files: list[Path] = []
        for dirpath, dirnames, filenames in os.walk(repo_root):
            dirnames[:] = [name for name in dirnames if name not in {".git", "target", "__pycache__"}]
            for filename in filenames:
                files.append(Path(dirpath) / filename)
        return files


def _should_index(path: Path, repo_root: Path) -> bool:
    rel = path.relative_to(repo_root)
    if any(part in {".git", "target", "vendor", "node_modules", "__pycache__"} for part in rel.parts):
        return False
    if path.suffix.lower() in TEXT_SUFFIXES:
        return True
    if path.name in {"Cargo.toml", "Cargo.lock", "README", "LICENSE"}:
        return True
    return path.suffix == ""


def _read_text(path: Path) -> str | None:
    try:
        raw = path.read_bytes()
    except Exception:
        return None
    if b"\0" in raw:
        return None
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        try:
            return raw.decode("utf-8", errors="ignore")
        except Exception:
            return None


def _extract_summary(text: str) -> str:
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    if not lines:
        return ""
    preferred = [
        line
        for line in lines
        if line.startswith("//")
        or line.startswith("///")
        or line.startswith("# ")
        or line.startswith("pub ")
        or line.startswith("fn ")
        or line.startswith("struct ")
        or line.startswith("enum ")
    ]
    chosen = preferred[:4] if preferred else lines[:4]
    return " | ".join(chosen)[:240]


@dataclass
class FileDoc:
    path: str
    full_path: Path
    text: str
    tokens: int
    summary: str
    content_terms: Counter[str]
    path_terms: Counter[str]
    item_names: set[str] = field(default_factory=set)
    outgoing_refs: set[str] = field(default_factory=set)

    @property
    def length(self) -> int:
        return max(1, sum(self.content_terms.values()))


@dataclass
class RepoIndex:
    repo_root: Path
    docs: list[FileDoc]
    docs_by_path: dict[str, FileDoc]
    content_df: Counter[str]
    path_df: Counter[str]
    avg_doc_len: float
    graph: dict[str, set[str]]
    git_touches: dict[str, set[str]]
    basename_groups: dict[str, list[str]]
    realpath_to_paths: dict[str, list[str]]


@dataclass
class RankedFile:
    path: str
    score: float
    reasons: list[str]
    summary: str
    feature_scores: dict[str, float]


@dataclass
class RetrievalMetrics:
    recall_at_5: float
    recall_at_10: float
    precision_at_5: float
    precision_at_10: float
    first_hit_rank: int | None
    mrr: float
    full_coverage_at_5: bool
    full_coverage_at_10: bool


def _token_counter(text: str) -> Counter[str]:
    counter: Counter[str] = Counter()
    for raw in WORD_RE.findall(text):
        for token in _split_identifier(raw):
            if len(token) > 1:
                counter[token] += 1
    return counter


def _candidate_docs(repo_root: Path) -> list[FileDoc]:
    docs: list[FileDoc] = []
    for path in _iter_files(repo_root):
        if not _should_index(path, repo_root):
            continue
        text = _read_text(path)
        if text is None:
            continue
        rel = _normalize_repo_path(str(path.relative_to(repo_root)))
        docs.append(
            FileDoc(
                path=rel,
                full_path=path,
                text=text,
                tokens=count_tokens(text),
                summary=_extract_summary(text),
                content_terms=_token_counter(text),
                path_terms=_token_counter(rel),
            )
        )
    return docs


def _link_graph(docs: list[FileDoc]) -> dict[str, set[str]]:
    by_stem: dict[str, set[str]] = defaultdict(set)
    by_item: dict[str, set[str]] = defaultdict(set)
    graph: dict[str, set[str]] = defaultdict(set)

    for doc in docs:
        stem = Path(doc.path).stem.lower()
        if stem and stem not in {"mod", "lib", "main"}:
            by_stem[stem].add(doc.path)
        item_names = {name.lower() for name in ITEM_RE.findall(doc.text)}
        doc.item_names = item_names
        for name in item_names:
            by_item[name].add(doc.path)

    for doc in docs:
        refs: set[str] = set()
        for match in IMPORT_RE.findall(doc.text):
            cleaned = match.replace("{", " ").replace("}", " ").replace(",", " ")
            for token in WORD_RE.findall(cleaned):
                for piece in _split_identifier(token):
                    refs.add(piece)
        doc.outgoing_refs = refs
        candidates: set[str] = set()
        for ref in refs:
            candidates.update(by_stem.get(ref, set()))
            candidates.update(by_item.get(ref, set()))
        candidates.discard(doc.path)
        if candidates:
            graph[doc.path].update(candidates)
            for other in candidates:
                graph[other].add(doc.path)
    return {path: set(neighbors) for path, neighbors in graph.items()}


def _build_git_touches(repo_root: Path, docs: list[FileDoc]) -> dict[str, set[str]]:
    if not (repo_root / ".git").exists():
        return {}
    candidate_paths = {doc.path for doc in docs}
    completed = subprocess.run(
        ["git", "-C", str(repo_root), "log", "--name-only", "--format=commit:%H", "--", "."],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        return {}
    touches: dict[str, set[str]] = defaultdict(set)
    current_commit: str | None = None
    for raw_line in completed.stdout.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith("commit:"):
            current_commit = line.split(":", 1)[1]
            continue
        if current_commit is None:
            continue
        path = _normalize_repo_path(line)
        if path in candidate_paths:
            touches[path].add(current_commit)
    return touches


@lru_cache(maxsize=8)
def load_repo_index(repo_root_text: str) -> RepoIndex:
    repo_root = Path(repo_root_text).resolve()
    docs = _candidate_docs(repo_root)
    docs_by_path = {doc.path: doc for doc in docs}
    content_df: Counter[str] = Counter()
    path_df: Counter[str] = Counter()
    for doc in docs:
        for token in doc.content_terms:
            content_df[token] += 1
        for token in doc.path_terms:
            path_df[token] += 1
    graph = _link_graph(docs)
    git_touches = _build_git_touches(repo_root, docs)
    basename_groups: dict[str, list[str]] = defaultdict(list)
    realpath_to_paths: dict[str, list[str]] = defaultdict(list)
    for doc in docs:
        basename_groups[Path(doc.path).name].append(doc.path)
        realpath_to_paths[str(doc.full_path.resolve())].append(doc.path)
    avg_doc_len = mean(doc.length for doc in docs) if docs else 1.0
    return RepoIndex(
        repo_root=repo_root,
        docs=docs,
        docs_by_path=docs_by_path,
        content_df=content_df,
        path_df=path_df,
        avg_doc_len=avg_doc_len,
        graph=graph,
        git_touches=git_touches,
        basename_groups=dict(basename_groups),
        realpath_to_paths=dict(realpath_to_paths),
    )


def _bm25(tf: int, df: int, doc_len: int, avg_len: float, total_docs: int) -> float:
    if tf <= 0 or df <= 0 or total_docs <= 0:
        return 0.0
    k1 = 1.2
    b = 0.75
    idf = math.log(1.0 + (total_docs - df + 0.5) / (df + 0.5))
    return idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * doc_len / max(avg_len, 1.0)))


def _lexical_scores(index: RepoIndex, query: str) -> tuple[dict[str, float], dict[str, dict[str, float]]]:
    terms, exact_terms = _query_terms(query)
    total_docs = len(index.docs)
    scores: dict[str, float] = {}
    features: dict[str, dict[str, float]] = {}
    test_hint = "test" in terms or "tests" in query.lower() or "verify" in query.lower()
    for doc in index.docs:
        content_score = 0.0
        path_score = 0.0
        exact_score = 0.0
        for term in terms:
            content_score += _bm25(
                doc.content_terms.get(term, 0),
                index.content_df.get(term, 0),
                doc.length,
                index.avg_doc_len,
                total_docs,
            )
            path_score += 2.5 * _bm25(
                doc.path_terms.get(term, 0),
                index.path_df.get(term, 0),
                max(1, sum(doc.path_terms.values())),
                6.0,
                total_docs,
            )
        text_lower = doc.text.lower()
        path_lower = doc.path.lower()
        for raw in exact_terms:
            lowered = raw.lower()
            if lowered in text_lower:
                exact_score += 4.0
            if lowered in path_lower:
                exact_score += 6.0
        if test_hint and any(part in doc.path for part in ("test", "tests")):
            path_score += 0.6
        total = content_score + path_score + exact_score
        if total > 0:
            scores[doc.path] = total
        features[doc.path] = {
            "content": content_score,
            "path": path_score,
            "exact": exact_score,
        }
    return scores, features


def _normalize_scores(raw_scores: dict[str, float]) -> dict[str, float]:
    if not raw_scores:
        return {}
    maximum = max(raw_scores.values())
    if maximum <= 0:
        return {path: 0.0 for path in raw_scores}
    return {path: value / maximum for path, value in raw_scores.items()}


def _scope_scores(index: RepoIndex, lexical_scores: dict[str, float]) -> tuple[dict[str, float], dict[str, dict[str, float]]]:
    normalized_lexical = _normalize_scores(lexical_scores)
    top_seeds = sorted(
        lexical_scores.items(),
        key=lambda item: (-item[1], item[0]),
    )[:8]
    graph_scores: dict[str, float] = defaultdict(float)
    for seed_path, seed_score in top_seeds:
        for neighbor in index.graph.get(seed_path, set()):
            graph_scores[neighbor] += seed_score
            for second_neighbor in index.graph.get(neighbor, set()):
                if second_neighbor != seed_path:
                    graph_scores[second_neighbor] += seed_score * 0.35

    cochange_scores: dict[str, float] = defaultdict(float)
    for seed_path, seed_score in top_seeds:
        seed_touches = index.git_touches.get(seed_path)
        if not seed_touches:
            continue
        for candidate_path, candidate_touches in index.git_touches.items():
            if candidate_path == seed_path or not candidate_touches:
                continue
            overlap = len(seed_touches & candidate_touches)
            if overlap == 0:
                continue
            cochange_scores[candidate_path] += seed_score * overlap / math.sqrt(
                len(seed_touches) * len(candidate_touches)
            )

    mirror_scores: dict[str, float] = defaultdict(float)
    for seed_path, seed_score in top_seeds:
        basename = Path(seed_path).name
        if basename in {"mod.rs", "lib.rs", "main.rs", "build.rs"}:
            continue
        siblings = index.basename_groups.get(basename, [])
        if len(siblings) < 2 or len(siblings) > 4:
            continue
        for sibling_path in siblings:
            if sibling_path != seed_path:
                mirror_scores[sibling_path] += seed_score * 0.85

    normalized_graph = _normalize_scores(graph_scores)
    normalized_cochange = _normalize_scores(cochange_scores)
    normalized_mirror = _normalize_scores(mirror_scores)

    combined: dict[str, float] = {}
    feature_scores: dict[str, dict[str, float]] = {}
    for doc in index.docs:
        lexical = normalized_lexical.get(doc.path, 0.0)
        graph = normalized_graph.get(doc.path, 0.0)
        cochange = normalized_cochange.get(doc.path, 0.0)
        mirror = normalized_mirror.get(doc.path, 0.0)
        combined[doc.path] = (0.62 * lexical) + (0.14 * graph) + (0.08 * cochange) + (0.16 * mirror)
        feature_scores[doc.path] = {
            "lexical": lexical,
            "graph": graph,
            "cochange": cochange,
            "mirror": mirror,
        }
    return combined, feature_scores


def _ranked_files(
    index: RepoIndex,
    query: str,
    *,
    mode: str,
    max_files: int,
) -> list[RankedFile]:
    lexical_scores, lexical_features = _lexical_scores(index, query)
    if mode == "lexical":
        combined = _normalize_scores(lexical_scores)
        mode_features = {path: {"lexical": score} for path, score in combined.items()}
    else:
        combined, scope_features = _scope_scores(index, lexical_scores)
        mode_features = scope_features
        for path, feature_map in mode_features.items():
            feature_map["lexical_raw"] = lexical_features.get(path, {}).get("content", 0.0) + lexical_features.get(path, {}).get("path", 0.0) + lexical_features.get(path, {}).get("exact", 0.0)

    ranked: list[RankedFile] = []
    for path, score in sorted(combined.items(), key=lambda item: (-item[1], item[0])):
        if score <= 0:
            continue
        doc = index.docs_by_path[path]
        features = mode_features.get(path, {})
        reasons: list[str] = []
        if lexical_features.get(path, {}).get("exact", 0.0) > 0:
            reasons.append("exact symbol match")
        if lexical_features.get(path, {}).get("path", 0.0) > 0:
            reasons.append("path/keyword match")
        if mode == "scope" and features.get("graph", 0.0) > 0.05:
            reasons.append("import-neighbor signal")
        if mode == "scope" and features.get("cochange", 0.0) > 0.05:
            reasons.append("git co-change signal")
        if mode == "scope" and features.get("mirror", 0.0) > 0.05:
            reasons.append("sibling-file signal")
        ranked.append(
            RankedFile(
                path=path,
                score=score,
                reasons=reasons or ["content keyword match"],
                summary=doc.summary,
                feature_scores=features,
            )
        )
        if len(ranked) >= max_files:
            break
    return ranked


def _metrics(ranked_paths: list[str], ground_truth: set[str]) -> RetrievalMetrics:
    def hits_at(limit: int) -> int:
        return sum(1 for path in ranked_paths[:limit] if path in ground_truth)

    first_hit_rank: int | None = None
    for index, path in enumerate(ranked_paths, start=1):
        if path in ground_truth:
            first_hit_rank = index
            break

    ground_truth_count = max(1, len(ground_truth))
    return RetrievalMetrics(
        recall_at_5=hits_at(5) / ground_truth_count,
        recall_at_10=hits_at(10) / ground_truth_count,
        precision_at_5=hits_at(5) / 5.0,
        precision_at_10=hits_at(10) / 10.0,
        first_hit_rank=first_hit_rank,
        mrr=(1.0 / first_hit_rank) if first_hit_rank else 0.0,
        full_coverage_at_5=ground_truth.issubset(set(ranked_paths[:5])),
        full_coverage_at_10=ground_truth.issubset(set(ranked_paths[:10])),
    )


def _top_read_budget(index: RepoIndex, ranked_paths: list[str], limit: int) -> int:
    total = 0
    for path in ranked_paths[:limit]:
        doc = index.docs_by_path.get(path)
        if doc is not None:
            total += doc.tokens
    return total


def _load_manifest(path: Path) -> list[dict[str, object]]:
    return json.loads(path.read_text())


def _report_path(ground_truth_root: Path, run_id: str) -> Path:
    return ground_truth_root / f"{run_id}.report.json"


def _canonicalize_ground_truth(index: RepoIndex, repo_root: Path, paths: list[str]) -> set[str]:
    canonical: set[str] = set()
    for raw_path in paths:
        normalized = _normalize_repo_path(raw_path)
        if normalized in index.docs_by_path:
            canonical.add(normalized)
            continue
        candidate = (repo_root / normalized).resolve()
        for mapped_path in index.realpath_to_paths.get(str(candidate), []):
            canonical.add(mapped_path)
    return canonical


def _verdict(aggregate: dict[str, object]) -> str:
    delta_recall_5 = float(aggregate["scope_avg_recall_at_5"]) - float(aggregate["lexical_avg_recall_at_5"])
    delta_coverage_10 = int(aggregate["scope_full_coverage_at_10_runs"]) - int(
        aggregate["lexical_full_coverage_at_10_runs"]
    )
    if delta_recall_5 >= 0.15 or delta_coverage_10 >= 2:
        return "PASS"
    if delta_recall_5 >= 0.05 or delta_coverage_10 >= 1:
        return "MARGINAL"
    return "FAIL"


def run_evaluation(
    *,
    manifest_path: Path = DEFAULT_MANIFEST,
    ground_truth_root: Path = DEFAULT_GROUND_TRUTH_ROOT,
    results_root: Path | None = None,
    max_files: int = 10,
) -> dict[str, object]:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    output_root = (results_root or DEFAULT_RESULTS_ROOT / f"study-{timestamp}").resolve()
    output_root.mkdir(parents=True, exist_ok=True)

    manifest = _load_manifest(manifest_path)
    runs: list[dict[str, object]] = []
    for item in manifest:
        run_id = str(item["run_id"])
        report_path = _report_path(ground_truth_root, run_id)
        report = json.loads(report_path.read_text())
        repo_path = (Path(str(item["repo_path"])) if Path(str(item["repo_path"])).is_absolute() else (manifest_path.parent.parent / str(item["repo_path"]))).resolve()
        if not repo_path.exists():
            repo_path = Path(str(item["repo_path"])).resolve()
        index = load_repo_index(str(repo_path))
        ground_truth = _canonicalize_ground_truth(index, repo_path, report["actual"]["edited_files"])
        lexical_ranked = _ranked_files(index, str(item["prompt"]), mode="lexical", max_files=max_files)
        scope_ranked = _ranked_files(index, str(item["prompt"]), mode="scope", max_files=max_files)
        lexical_paths = [row.path for row in lexical_ranked]
        scope_paths = [row.path for row in scope_ranked]
        lexical_metrics = _metrics(lexical_paths, ground_truth)
        scope_metrics = _metrics(scope_paths, ground_truth)
        payload = {
            "run_id": run_id,
            "task": item["title"],
            "prompt": item["prompt"],
            "repo_root": str(repo_path),
            "ground_truth_files": sorted(ground_truth),
            "actual_navigation_tokens": report["actual"]["navigation_tokens"],
            "lexical": {
                "metrics": lexical_metrics.__dict__,
                "top_files": [row.__dict__ for row in lexical_ranked],
                "top5_read_budget": _top_read_budget(index, lexical_paths, 5),
                "top10_read_budget": _top_read_budget(index, lexical_paths, 10),
            },
            "scope": {
                "metrics": scope_metrics.__dict__,
                "top_files": [row.__dict__ for row in scope_ranked],
                "top5_read_budget": _top_read_budget(index, scope_paths, 5),
                "top10_read_budget": _top_read_budget(index, scope_paths, 10),
            },
        }
        json_path = output_root / f"{run_id}.report.json"
        md_path = output_root / f"{run_id}.report.md"
        json_path.write_text(json.dumps(payload, indent=2) + "\n")
        md_path.write_text(render_run_report(payload))
        runs.append(payload)

    aggregate = aggregate_results(runs)
    aggregate["verdict"] = _verdict(aggregate)
    (output_root / "aggregate.json").write_text(json.dumps(aggregate, indent=2) + "\n")
    (output_root / "summary.md").write_text(render_summary(aggregate, runs, output_root.name))
    return {
        "results_root": str(output_root),
        "aggregate": aggregate,
        "summary_path": str(output_root / "summary.md"),
    }


def aggregate_results(runs: list[dict[str, object]]) -> dict[str, object]:
    def avg(path: str, mode: str) -> float:
        return mean(float(run[mode]["metrics"][path]) for run in runs) if runs else 0.0

    return {
        "total_runs": len(runs),
        "lexical_avg_recall_at_5": avg("recall_at_5", "lexical"),
        "lexical_avg_recall_at_10": avg("recall_at_10", "lexical"),
        "lexical_avg_mrr": avg("mrr", "lexical"),
        "lexical_full_coverage_at_10_runs": sum(
            1 for run in runs if run["lexical"]["metrics"]["full_coverage_at_10"]
        ),
        "scope_avg_recall_at_5": avg("recall_at_5", "scope"),
        "scope_avg_recall_at_10": avg("recall_at_10", "scope"),
        "scope_avg_mrr": avg("mrr", "scope"),
        "scope_full_coverage_at_10_runs": sum(
            1 for run in runs if run["scope"]["metrics"]["full_coverage_at_10"]
        ),
        "avg_scope_top5_read_budget": mean(int(run["scope"]["top5_read_budget"]) for run in runs)
        if runs
        else 0.0,
        "avg_scope_top10_read_budget": mean(int(run["scope"]["top10_read_budget"]) for run in runs)
        if runs
        else 0.0,
    }


def render_run_report(payload: dict[str, object]) -> str:
    lexical = payload["lexical"]
    scope = payload["scope"]
    lines = [
        "=== Scope Spike V2 Report ===",
        f"Run: {payload['run_id']}",
        f"Task: {payload['task']}",
        f"Repository: {payload['repo_root']}",
        f"Tokenizer: {tokenizer_metadata()['name']}",
        "",
        "GROUND TRUTH FILES:",
    ]
    lines.extend(f"- {path}" for path in payload["ground_truth_files"])
    lines.extend(
        [
            "",
            "LEXICAL BASELINE:",
            f"  Recall@5:          {lexical['metrics']['recall_at_5']:.1%}",
            f"  Recall@10:         {lexical['metrics']['recall_at_10']:.1%}",
            f"  MRR:               {lexical['metrics']['mrr']:.3f}",
            f"  Full coverage@10:  {lexical['metrics']['full_coverage_at_10']}",
            f"  Top-10 read budget:{lexical['top10_read_budget']}",
            "",
            "SCOPE RANKER:",
            f"  Recall@5:          {scope['metrics']['recall_at_5']:.1%}",
            f"  Recall@10:         {scope['metrics']['recall_at_10']:.1%}",
            f"  MRR:               {scope['metrics']['mrr']:.3f}",
            f"  Full coverage@10:  {scope['metrics']['full_coverage_at_10']}",
            f"  Top-10 read budget:{scope['top10_read_budget']}",
            "",
            "TOP SCOPE FILES:",
        ]
    )
    for row in scope["top_files"][:5]:
        lines.append(
            f"- {row['path']} ({', '.join(row['reasons'])}; score={row['score']:.3f})"
        )
    lines.append("")
    return "\n".join(lines)


def render_summary(aggregate: dict[str, object], runs: list[dict[str, object]], study_label: str) -> str:
    lines = [
        f"# Scope Spike V2 Summary ({study_label})",
        "",
        f"- Total runs: {aggregate['total_runs']}",
        f"- Lexical avg Recall@5: {aggregate['lexical_avg_recall_at_5']:.1%}",
        f"- Scope avg Recall@5: {aggregate['scope_avg_recall_at_5']:.1%}",
        f"- Lexical avg Recall@10: {aggregate['lexical_avg_recall_at_10']:.1%}",
        f"- Scope avg Recall@10: {aggregate['scope_avg_recall_at_10']:.1%}",
        f"- Lexical avg MRR: {aggregate['lexical_avg_mrr']:.3f}",
        f"- Scope avg MRR: {aggregate['scope_avg_mrr']:.3f}",
        f"- Lexical full coverage@10 runs: {aggregate['lexical_full_coverage_at_10_runs']}",
        f"- Scope full coverage@10 runs: {aggregate['scope_full_coverage_at_10_runs']}",
        f"- Scope avg top-5 read budget: {aggregate['avg_scope_top5_read_budget']:.0f} tokens",
        f"- Scope avg top-10 read budget: {aggregate['avg_scope_top10_read_budget']:.0f} tokens",
        f"- Incremental verdict over lexical baseline: {aggregate['verdict']}",
        "",
        "## Runs",
    ]
    for run in runs:
        lines.append(
            f"- {run['run_id']}: lexical R@10 {run['lexical']['metrics']['recall_at_10']:.1%}, "
            f"scope R@10 {run['scope']['metrics']['recall_at_10']:.1%}, "
            f"scope full@10={run['scope']['metrics']['full_coverage_at_10']}"
        )
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Evaluate a real Scope-style retriever against study traces.")
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST), help="Path to the study manifest.")
    parser.add_argument(
        "--ground-truth-root",
        default=str(DEFAULT_GROUND_TRUTH_ROOT),
        help="Directory containing the original scope_spike run reports.",
    )
    parser.add_argument(
        "--results-root",
        help="Directory to write results into. Defaults to a timestamped directory under scope_spike_v2/results.",
    )
    parser.add_argument("--max-files", type=int, default=10, help="How many ranked files to emit per task.")
    args = parser.parse_args()
    result = run_evaluation(
        manifest_path=Path(args.manifest),
        ground_truth_root=Path(args.ground_truth_root),
        results_root=Path(args.results_root) if args.results_root else None,
        max_files=args.max_files,
    )
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
