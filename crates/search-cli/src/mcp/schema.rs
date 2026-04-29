//! Static JSON Schema definitions for each MCP tool.
//!
//! Schemas are hard-coded `serde_json::Value` literals because the surface
//! is small and freezing them by hand is the simplest way to
//! guarantee schema stability across releases.

use serde_json::{Value, json};

pub struct ToolDescriptor {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
}

pub const TOOLS: &[ToolDescriptor] = &[
    ToolDescriptor {
        name: "find_files",
        title: "Find files",
        description: "Locate files by path or filename using the TriSeek index. Use this instead of `ls`, `find`, `fd`, globbing, or `rg --files` for file discovery in this repository.",
        input_schema: find_files_schema,
    },
    ToolDescriptor {
        name: "search_content",
        title: "Search file content",
        description: "Search repository content (literal or regex) using the TriSeek backend. Use this instead of `rg`, `grep`, or ad-hoc `sed` scans for exact code search before falling back to shell tools.",
        input_schema: search_content_schema,
    },
    ToolDescriptor {
        name: "search_path_and_content",
        title: "Search with path narrowing",
        description: "First narrow by path glob, then search content. Prefer this over shell pipelines that combine `find`, globs, and `rg` when you already know the subtree or filename pattern.",
        input_schema: search_path_and_content_schema,
    },
    ToolDescriptor {
        name: "context_pack",
        title: "Build an intent-aware context pack",
        description: "Return a tiny, bounded starting set of ranked files/snippets for a bugfix or review goal. Use this before chaining several broad searches.",
        input_schema: context_pack_schema,
    },
    ToolDescriptor {
        name: "index_status",
        title: "Report TriSeek index status",
        description: "Report whether the TriSeek index exists and is healthy for this repository.",
        input_schema: empty_object_schema,
    },
    ToolDescriptor {
        name: "reindex",
        title: "Rebuild or update the TriSeek index",
        description: "Rebuild or update the TriSeek index. Use `incremental` for fast updates and `full` for a complete rebuild.",
        input_schema: reindex_schema,
    },
    ToolDescriptor {
        name: "memo_status",
        title: "Check file freshness in session cache",
        description: "Check whether files changed since this session last read them. Use this before re-reading files to avoid redundant tokens.",
        input_schema: memo_status_schema,
    },
    ToolDescriptor {
        name: "memo_session",
        title: "Inspect memo session stats",
        description: "Show Memo session state: tracked files, read counts, and estimated tokens saved.",
        input_schema: memo_session_schema,
    },
    ToolDescriptor {
        name: "memo_check",
        title: "Check single-file freshness before reread",
        description: "Before re-reading a file you already saw in this Codex session, call this tool. If `recommendation` is `skip_reread`, do not read the file again and rely on prior context. Only re-read when it returns `reread_with_diff` or `reread`.",
        input_schema: memo_check_schema,
    },
];

pub fn find_files_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Path or filename substring to search for."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "default": 20,
                "description": "Maximum number of results to return (default 20, hard cap 100)."
            },
            "force_refresh": {
                "type": "boolean",
                "default": false,
                "description": "Bypass duplicate-result reuse and execute the search again."
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

pub fn search_content_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Literal string or regex pattern to search for."
            },
            "mode": {
                "type": "string",
                "enum": ["literal", "regex"],
                "default": "literal",
                "description": "Search mode. Use `literal` for exact string search, `regex` for a regular expression."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "default": 20
            },
            "force_refresh": {
                "type": "boolean",
                "default": false,
                "description": "Bypass duplicate-result reuse and execute the search again."
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

pub fn search_path_and_content_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path_query": {
                "type": "string",
                "description": "Glob pattern restricting which files to search (e.g. `src/**/*.rs`)."
            },
            "content_query": {
                "type": "string",
                "description": "Literal string or regex pattern to search for."
            },
            "mode": {
                "type": "string",
                "enum": ["literal", "regex"],
                "default": "literal"
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "default": 20
            },
            "force_refresh": {
                "type": "boolean",
                "default": false,
                "description": "Bypass duplicate-result reuse and execute the search again."
            }
        },
        "required": ["path_query", "content_query"],
        "additionalProperties": false
    })
}

pub fn context_pack_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "goal": {
                "type": "string",
                "description": "Natural-language task goal, such as `fix auth panic`."
            },
            "intent": {
                "type": "string",
                "enum": ["bugfix", "review"],
                "default": "bugfix",
                "description": "Task intent used to tune ranking heuristics."
            },
            "budget_tokens": {
                "type": "integer",
                "minimum": 1,
                "maximum": 4000,
                "default": 1200,
                "description": "Approximate output token budget. Hard-capped at 4000."
            },
            "max_files": {
                "type": "integer",
                "minimum": 1,
                "maximum": 12,
                "default": 4,
                "description": "Maximum ranked files to include. Hard-capped at 12."
            },
            "changed_files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional repository-relative paths to boost, especially for review intent."
            }
        },
        "required": ["goal"],
        "additionalProperties": false
    })
}

pub fn empty_object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

pub fn reindex_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "enum": ["incremental", "full"],
                "default": "incremental"
            }
        },
        "additionalProperties": false
    })
}

pub fn memo_status_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "description": "Repository-relative file paths to check."
            },
            "session_id": {
                "type": "string",
                "description": "Optional session identifier. If omitted, MCP metadata/session defaults are used."
            }
        },
        "required": ["files"],
        "additionalProperties": false
    })
}

pub fn memo_session_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {
                "type": "string",
                "description": "Optional session identifier. If omitted, MCP metadata/session defaults are used."
            }
        },
        "additionalProperties": false
    })
}

pub fn memo_check_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Repository-relative path of the file to check."
            },
            "session_id": {
                "type": "string",
                "description": "Optional session identifier. If omitted, MCP metadata/session defaults are used."
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}
