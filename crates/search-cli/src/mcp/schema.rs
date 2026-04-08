//! Static JSON Schema definitions for each MCP tool.
//!
//! Schemas are hard-coded `serde_json::Value` literals because the surface
//! is small (5 tools) and freezing them by hand is the simplest way to
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
        description: "Find files by path or filename using the TriSeek index. Returns ranked paths. Use this instead of globbing or ls to locate files.",
        input_schema: find_files_schema,
    },
    ToolDescriptor {
        name: "search_content",
        title: "Search file content",
        description: "Search file content (literal or regex) across the repository using the TriSeek hybrid backend. Returns line-level matches with short previews. Use this instead of shell grep for exact code search.",
        input_schema: search_content_schema,
    },
    ToolDescriptor {
        name: "search_path_and_content",
        title: "Search with path narrowing",
        description: "Narrow to path constraints (glob) then search content. Use when you know which subtree to search.",
        input_schema: search_path_and_content_schema,
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
            }
        },
        "required": ["path_query", "content_query"],
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
