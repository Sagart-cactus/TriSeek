//! Structured error types for MCP tool responses.
//!
//! Tool execution errors are **not** JSON-RPC-level errors. Per the MCP spec
//! they are returned inside a successful `CallToolResult` with `isError: true`
//! and a structured JSON body in the `content` text block. Protocol-level
//! errors (malformed frames, unknown methods) use JSON-RPC errors instead.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(dead_code)] // Codes exist as part of the frozen public error schema.
pub enum McpErrorCode {
    IndexUnavailable,
    IndexStale,
    InvalidQuery,
    RepoNotDetected,
    RootRequired,
    BroadRootReindexDenied,
    BackendFailure,
    FallbackFailure,
    ConfigWriteFailed,
    ClientNotInstalled,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpToolError {
    pub version: &'static str,
    pub error: McpToolErrorBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpToolErrorBody {
    pub code: McpErrorCode,
    pub message: String,
    pub retryable: bool,
    pub suggested_action: String,
}

#[allow(dead_code)] // Some constructors are reserved for future tool handlers.
impl McpToolError {
    pub fn new(
        code: McpErrorCode,
        message: impl Into<String>,
        retryable: bool,
        suggested_action: impl Into<String>,
    ) -> Self {
        Self {
            version: "1",
            error: McpToolErrorBody {
                code,
                message: message.into(),
                retryable,
                suggested_action: suggested_action.into(),
            },
        }
    }

    pub fn repo_not_detected() -> Self {
        Self::new(
            McpErrorCode::RepoNotDetected,
            "TriSeek could not detect a repository root from the working directory",
            false,
            "Start the server with --repo <PATH>, set TRISEEK_REPO_ROOT, or run it from inside a git repository",
        )
    }

    pub fn root_required() -> Self {
        Self::new(
            McpErrorCode::RootRequired,
            "TriSeek MCP started without a safe default root",
            false,
            "Pass `root` with the smallest folder that should be searched",
        )
    }

    pub fn broad_root_reindex_denied(root: impl Into<String>) -> Self {
        Self::new(
            McpErrorCode::BroadRootReindexDenied,
            format!(
                "TriSeek refused to create or update an index for broad root {}",
                root.into()
            ),
            false,
            "Pass a narrower project folder, or search this broad folder without reindexing",
        )
    }

    pub fn index_unavailable() -> Self {
        Self::new(
            McpErrorCode::IndexUnavailable,
            "TriSeek index is unavailable for this repository",
            true,
            "Call the `reindex` tool or run `triseek build <PATH>`",
        )
    }

    pub fn invalid_query(reason: impl Into<String>) -> Self {
        Self::new(
            McpErrorCode::InvalidQuery,
            reason,
            false,
            "Provide a non-empty query string and a valid mode",
        )
    }

    pub fn backend_failure(message: impl Into<String>) -> Self {
        Self::new(
            McpErrorCode::BackendFailure,
            message,
            true,
            "Retry the query; if the error persists, run `triseek doctor`",
        )
    }
}
