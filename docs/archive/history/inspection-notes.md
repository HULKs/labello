# Historical native inspection notes

These host-specific helper recipes and past results are retained for context.
They are not prerequisites for current inspection. Use the maintained
[inspector guide](../../../apps/egui-mcp-inspector/README.md).

This account also has `~/.local/bin/labello-inspector-headless`, an untracked
convenience launcher. From a worktree root, explicitly select that checkout and
the allocated port:

```sh
LABELLO_INSPECTOR_ROOT="$PWD" LABELLO_INSPECTION_PORT=5721 \
  labello-inspector-headless --preset setup
```

Check the manifest exists under that root before using the shortcut. The
installed launcher falls back to the main checkout if it cannot find the
manifest; a screenshot from that fallback does not verify a worktree change.
On another account or machine, use the full Xvfb command above rather than
assuming this local launcher exists.

If the session cannot expose newly registered MCP tools, the server can also
be tested by a client speaking MCP JSON-RPC over its stdio transport. This
account's local `~/.local/share/labello-inspection/mcp_call.py` does that, taking
a JSON array of tool calls on stdin. It starts a separate `egui-mcp` process
per invocation; include `attach` at the start and `disconnect` at the end.
For example, with the app running on 5721:

```sh
python3 "$HOME/.local/share/labello-inspection/mcp_call.py" <<'JSON'
[
  {"tool":"attach","args":{"host":"127.0.0.1","port":5721}},
  {"tool":"query_tree","args":{"role":"Button","limit":20}},
  {"tool":"disconnect"}
]
JSON
```

For parallel runs with this helper, place a copy in each driver's artifact
directory; it writes `mcp-stderr.log` beside itself. Redirect its stdout to that
same directory. The helper and its smoke-test artifacts are installation
conveniences, not repository tools or substitutes for issue regression tests.

The local launcher supports concurrent instances through
`LABELLO_INSPECTION_PORT`; use a different value in each driver's command.
Keep allocation and process ownership in the orchestration handoff. A port
conflict is a failed launch, not permission to attach to or stop its occupant.

A historical check exercised two Setup instances concurrently on
ports 5721 and 5722 with separate Xvfb displays and MCP server processes. One
opened compact Settings at 390x844 while the other retained Setup at 1288x820.
PNG dimensions and independent widget trees were checked again after both
drivers finished. This verifies native process isolation on this host; it does
not establish subagent-host MCP isolation or parallel live-server behavior.
