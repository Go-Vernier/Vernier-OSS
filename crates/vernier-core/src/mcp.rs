//! The Model Context Protocol server behind `vernier mcp`.
//!
//! A coding agent launched by `vernier agent` asks Vernier questions over
//! stdio instead of re-deriving the service graph from source. The protocol
//! is small (newline-delimited JSON-RPC 2.0, three tools), so it is written
//! on `serde_json` rather than pulling in an MCP SDK.
use std::io::{self, BufRead, Write};

use serde_json::{Map, Value, json};

use crate::analyze::Analysis;
use crate::blast::{self, Change};
use crate::git;

/// The one protocol version spoken, whatever the client asks for.
const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_DEPTH: u64 = 10;

const INSTRUCTIONS: &str = "Vernier maps the services in this repository and which of them a change can reach. Call blast_radius to see what a change affects, services to list the services, and edges to see how one service connects to others, with evidence.";

/// A loaded analysis answering requests. Loaded once so every call is fast.
pub struct Server {
    analysis: Analysis,
}

impl Server {
    pub fn new(analysis: Analysis) -> Self {
        Self { analysis }
    }

    /// Reads one request per line until EOF and writes one reply per line.
    /// Stdout carries the protocol and nothing else.
    ///
    /// # Errors
    /// Fails when reading the input or writing the output fails.
    pub fn serve(mut self, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(msg) => self.handle(&msg),
                Err(e) => Some(error(&Value::Null, -32700, &format!("parse error: {e}"))),
            };
            if let Some(reply) = reply {
                writeln!(output, "{reply}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// The reply to one message, or None for a notification.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            return Some(error(
                &Value::Null,
                -32600,
                "invalid request: expected a JSON object",
            ));
        };
        let id = obj.get("id");
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            return Some(error(
                id.unwrap_or(&Value::Null),
                -32600,
                "invalid request: no method",
            ));
        };
        let id = id?;
        let params = obj.get("params");
        Some(match method {
            "initialize" => success(id, &initialize_result()),
            "ping" => success(id, &json!({})),
            "tools/list" => success(id, &json!({ "tools": tool_list() })),
            "tools/call" => success(id, &self.call(params)),
            other => error(id, -32601, &format!("method not found: {other}")),
        })
    }

    fn call(&self, params: Option<&Value>) -> Value {
        let name = params.and_then(|p| p.get("name")).and_then(Value::as_str);
        let empty = Map::new();
        let args = match params.and_then(|p| p.get("arguments")) {
            None | Some(Value::Null) => Ok(&empty),
            Some(Value::Object(map)) => Ok(map),
            Some(_) => Err("arguments must be an object".to_string()),
        };
        let result = match (name, args) {
            (_, Err(message)) => Err(message),
            (Some("blast_radius"), Ok(args)) => self.blast_radius(args),
            (Some("services"), Ok(args)) => self.services(args),
            (Some("edges"), Ok(args)) => self.edges(args),
            (Some(other), _) => Err(format!(
                "unknown tool {other}; tools are blast_radius, services and edges"
            )),
            (None, _) => Err("tools/call needs a tool name".to_string()),
        };
        match result {
            Ok(value) => tool_text(&pretty(&value), false),
            Err(message) => tool_text(&message, true),
        }
    }

    fn blast_radius(&self, args: &Map<String, Value>) -> Result<Value, String> {
        only_keys(args, &["files", "pr", "diff", "depth"])?;
        let given = ["files", "pr", "diff"]
            .iter()
            .filter(|k| args.contains_key(**k))
            .count();
        if given > 1 {
            return Err("pass at most one of files, pr and diff".into());
        }
        let depth = match args.get("depth") {
            None => blast::DEFAULT_DEPTH,
            Some(v) => match v.as_u64() {
                Some(n @ 1..=MAX_DEPTH) => usize::try_from(n).map_err(|e| e.to_string())?,
                _ => return Err(format!("depth must be an integer from 1 to {MAX_DEPTH}")),
            },
        };
        let root = &self.analysis.root;
        let change: Change = if let Some(files) = args.get("files") {
            let files: Vec<String> = files
                .as_array()
                .and_then(|a| a.iter().map(|f| f.as_str().map(str::to_string)).collect())
                .ok_or("files must be an array of repo-relative paths")?;
            if files.is_empty() {
                return Err("files is empty; name at least one path".into());
            }
            Change::from_files(&files)
        } else if let Some(pr) = args.get("pr") {
            let n = pr.as_u64().ok_or("pr must be a non-negative integer")?;
            git::pull_request(root, n).map_err(|e| e.to_string())?
        } else if let Some(diff) = args.get("diff") {
            let range = diff.as_str().ok_or("diff must be a string")?;
            git::diff(root, range).map_err(|e| e.to_string())?
        } else {
            git::working_change(root)
                .map_err(|e| e.to_string())?
                .ok_or("no changes on this branch; pass files, pr or diff")?
        };
        let blast = blast::of_change(&self.analysis.graph, change, depth);
        serde_json::to_value(&blast).map_err(|e| e.to_string())
    }

    fn services(&self, args: &Map<String, Value>) -> Result<Value, String> {
        only_keys(args, &[])?;
        let services = self
            .analysis
            .graph
            .services()
            .into_iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "role": s.role,
                    "language": s.language,
                    "root": s.root,
                })
            })
            .collect();
        Ok(Value::Array(services))
    }

    fn edges(&self, args: &Map<String, Value>) -> Result<Value, String> {
        only_keys(args, &["service"])?;
        let graph = &self.analysis.graph;
        let edges: Vec<&crate::model::Edge> = match args.get("service") {
            None | Some(Value::Null) => graph.edges().iter().collect(),
            Some(v) => {
                let name = v.as_str().ok_or("service must be a string")?;
                if !graph.has_service(name) {
                    return Err(format!(
                        "unknown service {name}; call services to list them"
                    ));
                }
                let mut touching = graph.outbound(name);
                touching.extend(graph.inbound(name));
                touching
            }
        };
        serde_json::to_value(&edges).map_err(|e| e.to_string())
    }
}

/// Rejects arguments the schema does not declare, as `additionalProperties:
/// false` promises.
fn only_keys(args: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    match args.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(extra) => Err(format!("unknown argument {extra}")),
        None => Ok(()),
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "vernier", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tool_list() -> Value {
    json!([
        {
            "name": "blast_radius",
            "description": "The services a change can reach, walking the dependency graph from the services that own the changed files. Pass one of files, pr or diff; with none, the working change on this branch is used.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Changed files, relative to the repository root"
                    },
                    "pr": { "type": "integer", "description": "A pull request number found in the local git history" },
                    "diff": { "type": "string", "description": "A git diff range such as main...HEAD" },
                    "depth": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_DEPTH,
                        "description": "How many hops to follow; default 3"
                    }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "services",
            "description": "Every service in the repository with its role, language and root directory.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "edges",
            "description": "The dependency edges between services, with their evidence and confidence. Pass service to see only the edges into or out of it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "service": { "type": "string", "description": "A service name from the services tool" }
                },
                "additionalProperties": false
            }
        }
    ])
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn tool_text(text: &str, is_error: bool) -> Value {
    let mut result = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error {
        result["isError"] = Value::Bool(true);
    }
    result
}

fn success(id: &Value, result: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::analyze;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn server(fixture: &str) -> Server {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(fixture);
        Server::new(analyze(&root).unwrap())
    }

    fn call(s: &mut Server, name: &str, arguments: &Value) -> Value {
        let req = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}});
        s.handle(&req).unwrap()["result"].clone()
    }

    fn text(result: &Value) -> &str {
        result["content"][0]["text"].as_str().unwrap()
    }

    #[test]
    fn initialize_reports_tools_capability() {
        let mut s = server("compose-app");
        let req = json!({"jsonrpc":"2.0","id":7,"method":"initialize",
            "params":{"protocolVersion":"1999-01-01"}});
        let reply = s.handle(&req).unwrap();
        assert_eq!(reply["id"], 7);
        let r = &reply["result"];
        assert_eq!(r["protocolVersion"], "2025-06-18");
        assert!(r["capabilities"]["tools"].is_object());
        assert_eq!(r["serverInfo"]["name"], "vernier");
        assert!(r["instructions"].as_str().is_some_and(|i| !i.is_empty()));
    }

    #[test]
    fn initialized_notification_gets_no_reply() {
        let mut s = server("compose-app");
        let note = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert_eq!(s.handle(&note), None);
    }

    #[test]
    fn ping_gets_an_empty_result() {
        let mut s = server("compose-app");
        let reply = s
            .handle(&json!({"jsonrpc":"2.0","id":"a","method":"ping"}))
            .unwrap();
        assert_eq!(reply["result"], json!({}));
        assert_eq!(reply["id"], "a");
    }

    #[test]
    fn tools_list_names_three_tools() {
        let mut s = server("compose-app");
        let reply = s
            .handle(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .unwrap();
        let tools = reply["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["blast_radius", "services", "edges"]);
        for t in tools {
            assert!(t["description"].as_str().is_some_and(|d| !d.is_empty()));
            let schema = &t["inputSchema"];
            assert_eq!(schema["type"], "object");
            assert!(schema["properties"].is_object());
            assert_eq!(schema["additionalProperties"], false);
        }
    }

    #[test]
    fn unknown_method_is_a_jsonrpc_error() {
        let mut s = server("compose-app");
        let reply = s
            .handle(&json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}))
            .unwrap();
        assert_eq!(reply["error"]["code"], -32601);
    }

    #[test]
    fn non_object_and_array_messages_are_invalid_requests() {
        let mut s = server("compose-app");
        for msg in [json!([1, 2]), json!(5), json!("x")] {
            let reply = s.handle(&msg).unwrap();
            assert_eq!(reply["error"]["code"], -32600);
            assert_eq!(reply["id"], Value::Null);
        }
    }

    #[test]
    fn bad_json_is_parse_error() {
        let s = server("compose-app");
        let mut out = Vec::new();
        s.serve("{not json\n".as_bytes(), &mut out).unwrap();
        let reply: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(reply["error"]["code"], -32700);
        assert_eq!(reply["id"], Value::Null);
    }

    #[test]
    fn serve_replies_one_line_each_and_skips_notifications() {
        let s = server("compose-app");
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n",
        );
        let mut out = Vec::new();
        s.serve(input.as_bytes(), &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap().lines().count(), 2);
    }

    #[test]
    fn blast_radius_for_files() {
        let mut s = server("compose-app");
        let result = call(
            &mut s,
            "blast_radius",
            &json!({"files":["services/orders/main.go"],"depth":2}),
        );
        assert_eq!(result.get("isError"), None);
        let blast: Value = serde_json::from_str(text(&result)).unwrap();
        assert_eq!(blast["changed"][0]["service"], "orders");
        assert_eq!(blast["depth"], 2);
    }

    #[test]
    fn blast_radius_rejects_more_than_one_change_and_bad_depth() {
        let mut s = server("compose-app");
        let both = call(
            &mut s,
            "blast_radius",
            &json!({"files":["a"],"diff":"HEAD"}),
        );
        assert_eq!(both["isError"], true);
        for depth in [json!(0), json!(11), json!("2")] {
            let r = call(
                &mut s,
                "blast_radius",
                &json!({"files":["a"],"depth":depth}),
            );
            assert_eq!(r["isError"], true, "{depth}");
        }
        let extra = call(&mut s, "blast_radius", &json!({"files":["a"],"x":1}));
        assert_eq!(extra["isError"], true);
    }

    #[test]
    fn services_lists_name_role_language_and_root() {
        let mut s = server("compose-app");
        let result = call(&mut s, "services", &json!({}));
        let list: Value = serde_json::from_str(text(&result)).unwrap();
        let orders = list
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "orders")
            .unwrap();
        assert_eq!(orders["role"], "code");
        assert_eq!(orders["root"], "services/orders");
        assert!(orders.get("language").is_some());
    }

    #[test]
    fn edges_filters_by_service() {
        let mut s = server("edges-http-app");
        let all: Value = serde_json::from_str(text(&call(&mut s, "edges", &json!({})))).unwrap();
        let some: Value =
            serde_json::from_str(text(&call(&mut s, "edges", &json!({"service":"cart"})))).unwrap();
        let some = some.as_array().unwrap();
        assert_eq!(some.len(), 4, "cart's outbound and inbound edges");
        assert!(some.len() < all.as_array().unwrap().len());
        assert!(
            some.iter()
                .all(|e| e["source"] == "cart" || e["target"] == "cart")
        );
        assert!(some[0]["evidence"].is_array());
    }

    #[test]
    fn unknown_service_edges_is_a_tool_error() {
        let mut s = server("compose-app");
        let r = call(&mut s, "edges", &json!({"service":"nope"}));
        assert_eq!(r["isError"], true);
        assert_eq!(text(&r), "unknown service nope; call services to list them");
    }

    #[test]
    fn unknown_tool_is_a_tool_error_not_a_jsonrpc_error() {
        let mut s = server("compose-app");
        let reply = s
            .handle(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"nope"}}))
            .unwrap();
        assert!(reply.get("error").is_none());
        assert_eq!(reply["result"]["isError"], true);
    }

    #[test]
    fn a_diff_with_an_option_is_a_tool_error_and_writes_nothing() {
        let mut s = server("compose-app");
        let out = std::env::temp_dir().join(format!("vernier-mcp-pwn-{}", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let diff = format!("--output={} HEAD~1 HEAD", out.display());
        let r = call(&mut s, "blast_radius", &json!({ "diff": diff }));
        assert_eq!(r["isError"], true);
        assert!(text(&r).contains("not options"), "{}", text(&r));
        assert!(!out.exists());
    }
}
