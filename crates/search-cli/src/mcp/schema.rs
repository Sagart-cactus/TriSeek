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
        description: "Locate files by path or filename. Use this instead of `ls`, `find`, `fd`, globbing, or `rg --files`. Pass `root` as the smallest folder to search, especially when MCP was started by an app.",
        input_schema: find_files_schema,
    },
    ToolDescriptor {
        name: "search_content",
        title: "Search file content",
        description: "Search file content with literal or regex mode. Pass `root` as the smallest folder to search, especially when MCP was started by an app.",
        input_schema: search_content_schema,
    },
    ToolDescriptor {
        name: "search_path_and_content",
        title: "Search with path narrowing",
        description: "First narrow by path glob, then search content. Pass `root` as the smallest folder to search.",
        input_schema: search_path_and_content_schema,
    },
    ToolDescriptor {
        name: "context_pack",
        title: "Build an intent-aware context pack",
        description: "Return a tiny, bounded starting set of ranked files/snippets for a bugfix or review goal. Pass `root` as the smallest relevant folder.",
        input_schema: context_pack_schema,
    },
    ToolDescriptor {
        name: "index_status",
        title: "Report TriSeek index status",
        description: "Report whether the TriSeek index exists and is healthy for a root.",
        input_schema: root_only_schema,
    },
    ToolDescriptor {
        name: "reindex",
        title: "Rebuild or update the TriSeek index",
        description: "Rebuild or update the TriSeek index for a narrow project root. Broad roots are rejected by default.",
        input_schema: reindex_schema,
    },
    ToolDescriptor {
        name: "usage_metrics",
        title: "Report privacy-preserving usage metrics",
        description: "Return local aggregate MCP usage counters and cache sizes. Does not include query text, paths, file contents, session ids, user ids, or network telemetry.",
        input_schema: empty_object_schema,
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
    ToolDescriptor {
        name: "session_open",
        title: "Open a portable session",
        description: "Declare a session for cross-agent context capture and set it as the current MCP session.",
        input_schema: session_open_schema,
    },
    ToolDescriptor {
        name: "session_status",
        title: "Inspect current session",
        description: "Return session state and action-log size for a portable session.",
        input_schema: session_id_schema,
    },
    ToolDescriptor {
        name: "session_list",
        title: "List portable sessions",
        description: "List portable sessions for the current repository.",
        input_schema: root_only_schema,
    },
    ToolDescriptor {
        name: "session_close",
        title: "Close portable session",
        description: "Mark a portable session as resolved or abandoned.",
        input_schema: session_close_schema,
    },
    ToolDescriptor {
        name: "session_snapshot",
        title: "Create session snapshot",
        description: "Persist a session snapshot directory under the TriSeek daemon snapshots directory.",
        input_schema: session_snapshot_schema,
    },
    ToolDescriptor {
        name: "session_snapshot_list",
        title: "List session snapshots",
        description: "List snapshot manifests, optionally filtered by session id.",
        input_schema: snapshot_list_schema,
    },
    ToolDescriptor {
        name: "session_snapshot_get",
        title: "Get session snapshot",
        description: "Return a full session snapshot including manifest, working set, action log, and pinned snippets.",
        input_schema: snapshot_get_schema,
    },
    ToolDescriptor {
        name: "session_snapshot_diff",
        title: "Diff session snapshots",
        description: "Compare two snapshots and report changed files and searches.",
        input_schema: snapshot_diff_schema,
    },
    ToolDescriptor {
        name: "session_resume",
        title: "Prepare session resume",
        description: "Hydrate daemon state from a snapshot id or .tcp pack and return a markdown payload for the new harness.",
        input_schema: session_resume_schema,
    },
    ToolDescriptor {
        name: "session_handoff",
        title: "Create handoff snapshot",
        description: "Convenience wrapper that creates a session snapshot, optionally writes a .tcp pack, and closes the current session as resolved.",
        input_schema: session_handoff_schema,
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
            "root": root_property(),
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
            "root": root_property(),
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
            "root": root_property(),
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
                "description": "Natural-language session goal, such as `fix auth panic`."
            },
            "root": root_property(),
            "intent": {
                "type": "string",
                "enum": ["bugfix", "review"],
                "default": "bugfix",
                "description": "Session intent used to tune ranking heuristics."
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

pub fn root_only_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property()
        },
        "additionalProperties": false
    })
}

fn root_property() -> Value {
    json!({
        "type": "string",
        "description": "Absolute or relative folder to search. Use the smallest relevant project or workspace folder. Broad roots use direct search only and cannot be reindexed."
    })
}

pub fn reindex_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
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
            "root": root_property(),
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
            "root": root_property(),
            "session_id": {
                "type": "string",
                "description": "Optional session identifier. If omitted, MCP metadata/session defaults are used."
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

pub fn session_open_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "session_id": {"type": "string"},
            "goal": {"type": "string", "default": ""}
        },
        "additionalProperties": false
    })
}

pub fn session_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "session_id": {"type": "string"}
        },
        "additionalProperties": false
    })
}

pub fn session_close_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "session_id": {"type": "string"},
            "status": {"type": "string", "enum": ["resolved", "abandoned"], "default": "resolved"}
        },
        "additionalProperties": false
    })
}

pub fn session_snapshot_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "session_id": {"type": "string"},
            "source_harness": {"type": "string"},
            "source_model": {"type": "string"},
            "pinned_snippet_paths": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "line_start": {"type": "integer", "minimum": 1},
                        "line_end": {"type": "integer", "minimum": 1}
                    },
                    "required": ["path", "line_start", "line_end"],
                    "additionalProperties": false
                }
            }
        },
        "additionalProperties": false
    })
}

pub fn session_handoff_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "session_id": {"type": "string"},
            "source_harness": {"type": "string"},
            "source_model": {"type": "string"},
            "pinned_snippet_paths": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "line_start": {"type": "integer", "minimum": 1},
                        "line_end": {"type": "integer", "minimum": 1}
                    },
                    "required": ["path", "line_start", "line_end"],
                    "additionalProperties": false
                }
            },
            "mode": {"type": "string", "enum": ["metadata", "git"], "default": "metadata"},
            "target_harness": {"type": "string", "description": "Target harness for handoff metadata, for example codex or claude."},
            "pack_output_path": {"type": "string", "description": "Optional .tcp output path. Relative paths resolve against the MCP repo root."},
            "branch": {"type": "string", "description": "Git handoff branch for mode=git."},
            "remote": {"type": "string", "description": "Git remote for mode=git."},
            "message": {"type": "string", "description": "Git commit message for mode=git."},
            "continue_existing": {"type": "boolean", "default": false}
        },
        "additionalProperties": false
    })
}

pub fn snapshot_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "session_id": {"type": "string"}
        },
        "additionalProperties": false
    })
}

pub fn snapshot_get_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "snapshot_id": {"type": "string"}
        },
        "required": ["snapshot_id"],
        "additionalProperties": false
    })
}

pub fn snapshot_diff_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "snapshot_a": {"type": "string"},
            "snapshot_b": {"type": "string"}
        },
        "required": ["snapshot_a", "snapshot_b"],
        "additionalProperties": false
    })
}

pub fn session_resume_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "root": root_property(),
            "snapshot_id": {"type": "string", "description": "Snapshot id, or for compatibility a .tcp pack path."},
            "pack_path": {"type": "string", "description": "Optional .tcp pack path. Relative paths resolve against the MCP repo root."},
            "budget_tokens": {"type": "integer", "minimum": 1, "maximum": 12000}
        },
        "additionalProperties": false
    })
}
