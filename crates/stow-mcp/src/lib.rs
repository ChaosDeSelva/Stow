//! MCP (Model Context Protocol) server over a Stow library, for `stw mcp`.
//!
//! [`StowMcp`] exposes the library to an MCP client (Claude Desktop, Claude Code, …) as
//! tools (`search`, `get_item`, `add_links`, …) and resources (`stow://item/{id}`,
//! `stow://bucket/{id}`). It speaks to the same SQLite database as the app and the CLI
//! and does no background work of its own: `add_links` fetches inline or leaves the
//! snapshot `pending` for the app to resume.
//!
//! Read-only mode ([`StowMcp::new`] with `read_only = true`, or `STOW_MCP_READ_ONLY=1`)
//! hides every tool that writes; a call to one is refused as an unknown tool.
#![forbid(unsafe_code)]

mod tools;
mod view;

use std::future::{ready, Future};
use std::io;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    ErrorData as McpError, Implementation, ListResourceTemplatesResult, ListResourcesResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, ResourceTemplate, ServerCapabilities, ServerConfig,
};
use rmcp::service::{MaybeSendFuture, RequestContext, RoleServer, ServerInitializeError};
use rmcp::{tool_handler, ServerHandler, ServiceExt};
use stow_core::repo::buckets;
use stow_core::{Stow, StowError};

pub use view::{BucketView, ItemView, TEXT_LIMIT};

/// Tools that change the library (or write files); hidden in read-only mode.
pub const MUTATING_TOOLS: &[&str] = &[
    "add_links",
    "add_reminder",
    "add_task",
    "archive_item",
    "complete_task",
    "create_bucket",
    "delete_item",
    "export",
    "move_item",
    "update_notes",
];

const INSTRUCTIONS: &str = "Stow is the user's local read-later library: links parked in \
intent-named buckets (the Inbox holds unsorted ones), each with notes, tasks, reminders, a \
readable text snapshot, voice-memo transcripts and AI notes. Start with `search` or \
`list_buckets`; `get_item` returns everything known about one link including its text. \
Ids are ULIDs; buckets may also be named by (case-insensitive) name, and `inbox` means the \
Inbox. `add_links` saves URLs (deduplicated by canonical URL) and, unless `fetch` is \
false, downloads their text right away.";

/// The MCP server: one Stow library plus the tool router.
#[derive(Debug)]
pub struct StowMcp {
    stow: Stow,
    read_only: bool,
    tool_router: ToolRouter<Self>,
}

impl StowMcp {
    /// Serve `stow`. With `read_only`, the [`MUTATING_TOOLS`] are neither listed nor callable.
    pub fn new(stow: Stow, read_only: bool) -> Self {
        let mut tool_router = Self::tool_router();
        if read_only {
            for name in MUTATING_TOOLS {
                tool_router.disable_route(*name);
            }
        }
        Self {
            stow,
            read_only,
            tool_router,
        }
    }

    /// Whether mutating tools are hidden.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    fn db(&self) -> &stow_core::Db {
        self.stow.db()
    }
}

/// `STOW_MCP_READ_ONLY` semantics: `1`, `true`, or `yes` (any case) turn read-only on.
pub fn read_only_env(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let v = v.trim();
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
    })
}

/// [`read_only_env`] applied to the process environment.
pub fn read_only_from_env() -> bool {
    read_only_env(std::env::var("STOW_MCP_READ_ONLY").ok().as_deref())
}

/// Serve `stow` on this process's stdin/stdout until the client disconnects. Nothing but
/// protocol traffic is written to stdout. A client that closes the pipe before the
/// handshake (for example `stw mcp < /dev/null`) is a normal, empty session.
pub async fn serve_stdio(stow: Stow, read_only: bool) -> io::Result<()> {
    let server = StowMcp::new(stow, read_only);
    let running = match server.serve(rmcp::transport::stdio()).await {
        Ok(running) => running,
        Err(ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
        Err(e) => return Err(io::Error::other(e)),
    };
    let quit = running.waiting().await.map_err(io::Error::other)?;
    tracing::info!("mcp session ended: {quit:?}");
    Ok(())
}

/// `-32602` for a URI outside `stow://item/{id}` and `stow://bucket/{id}`.
fn parse_uri(uri: &str) -> Result<(&str, &str), McpError> {
    uri.strip_prefix("stow://")
        .and_then(|rest| rest.split_once('/'))
        .filter(|(kind, id)| matches!(*kind, "item" | "bucket") && !id.is_empty())
        .ok_or_else(|| {
            McpError::invalid_params(
                format!("unsupported resource uri {uri:?}; use stow://item/{{id}} or stow://bucket/{{id}}"),
                None,
            )
        })
}

fn resource_error(uri: &str, e: &StowError) -> McpError {
    let msg = format!("{}: {e}", e.code());
    if e.is_not_found() {
        McpError::resource_not_found(msg, Some(serde_json::json!({ "uri": uri })))
    } else {
        McpError::internal_error(msg, None)
    }
}

#[tool_handler(router = self.tool_router)]
// The generated `list_tools` is `async` without an `.await`. Newer clippy reports this as
// `unused_async_trait_impl`, older as `unused_async`; allow both so every toolchain is clean.
#[allow(unknown_lints, clippy::unused_async, clippy::unused_async_trait_impl)]
impl ServerHandler for StowMcp {
    fn get_info(&self) -> ServerConfig {
        let mut instructions = INSTRUCTIONS.to_string();
        if self.read_only {
            instructions.push_str(" This server is read-only: nothing can be added or changed.");
        }
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("stow", env!("CARGO_PKG_VERSION")))
        .with_instructions(instructions)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + MaybeSendFuture + '_ {
        ready(
            buckets::list(self.db())
                .map_err(|e| resource_error("stow://bucket", &e))
                .map(|list| {
                    let resources = list
                        .into_iter()
                        .map(|b| {
                            let mut r = Resource::new(format!("stow://bucket/{}", b.id), b.name)
                                .with_mime_type("text/markdown");
                            if !b.description_md.trim().is_empty() {
                                r = r.with_description(b.description_md.trim().to_string());
                            }
                            r
                        })
                        .collect();
                    ListResourcesResult::with_all_items(resources)
                }),
        )
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, McpError>> + MaybeSendFuture + '_
    {
        ready(Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new("stow://item/{id}", "item")
                .with_description("A saved link as Markdown: metadata, notes, tasks, reminders, readable text, transcripts, AI notes")
                .with_mime_type("text/markdown"),
            ResourceTemplate::new("stow://bucket/{id}", "bucket")
                .with_description("A bucket as Markdown: description, notes, tasks, and its items")
                .with_mime_type("text/markdown"),
        ])))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResponse, McpError>> + MaybeSendFuture + '_ {
        ready(self.read_resource_sync(&request.uri))
    }
}

impl StowMcp {
    fn read_resource_sync(&self, uri: &str) -> Result<ReadResourceResponse, McpError> {
        let (kind, id) = parse_uri(uri)?;
        let markdown = match kind {
            "item" => ItemView::load(self.db(), id, true).map(|v| v.to_markdown()),
            _ => BucketView::load(self.db(), id).map(|v| v.to_markdown()),
        }
        .map_err(|e| resource_error(uri, &e))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(markdown, uri).with_mime_type("text/markdown")
        ])
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_parsing() {
        assert_eq!(parse_uri("stow://item/01A").unwrap(), ("item", "01A"));
        assert_eq!(parse_uri("stow://bucket/B").unwrap(), ("bucket", "B"));
        for bad in [
            "stow://item/",
            "stow://task/1",
            "file:///x",
            "stow://item",
            "",
        ] {
            assert!(parse_uri(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn read_only_disables_every_mutating_tool() {
        let dir = tempfile::tempdir().unwrap();
        let stow = Stow::open(dir.path()).unwrap();
        let rw = StowMcp::new(stow.clone(), false);
        let ro = StowMcp::new(stow, true);
        assert!(!rw.read_only() && ro.read_only());
        let names = |s: &StowMcp| -> Vec<String> {
            s.tool_router
                .list_all()
                .iter()
                .map(|t| t.name.to_string())
                .collect()
        };
        let all = names(&rw);
        let read = names(&ro);
        for m in MUTATING_TOOLS {
            assert!(all.iter().any(|n| n == m), "{m} missing from the router");
            assert!(!read.iter().any(|n| n == m), "{m} listed in read-only mode");
        }
        assert_eq!(all.len(), read.len() + MUTATING_TOOLS.len());
        assert!(ro.get_info().instructions.unwrap().contains("read-only"));
    }
}
