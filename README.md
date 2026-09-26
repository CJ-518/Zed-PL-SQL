[English](README.md) | [Español](README.es.md)

# Zed-PL-SQL

Zed extension wiring [plsqllang-server](https://github.com/EwanDubashinski/plsqllang-server) (a PL/SQL LSP for syntax checking) into Zed as a language server for SQL files.

A small proxy (`plsqllang-proxy`) sits between Zed and the real `plsqllang-server`. Because the upstream parser's grammar only checks a single top-level statement per document, the proxy splits each buffer into one virtual document per top-level SQL/PL-SQL statement, sends each to the real server independently, and remaps + merges the diagnostics it gets back onto the real document's line numbers — so every statement in a multi-statement script gets checked, not just the first. The proxy also filters out known false-positive diagnostics for SQL\*Plus/SQLcl client-side directives that the parser doesn't understand.

## Prerequisites

You need a `plsqllang-server` binary on your PATH. The easiest source is the
`server-all.jar` bundled inside the [plsqllang-client VS Code extension](https://marketplace.visualstudio.com/items?itemName=EwanDubashinski.plsqllint)
`.vsix` file (under `extension/server/server-all.jar`) — building the
`plsqllang-server` repo from source directly currently fails, since it depends
on a `parser` module that isn't publicly published.

Requires a JDK (1.8+).

### Windows
Create `plsqllang-server.bat` somewhere on your PATH:
```bat
@echo off
java -jar "C:\path\to\server-all.jar" %*
```

### macOS / Linux
Create an executable `plsqllang-server` shell script somewhere on your PATH:
```bash
#!/usr/bin/env bash
exec java -jar /path/to/server-all.jar "$@"
```

You also need the `plsqllang-proxy` binary on your PATH. Build it from the `proxy/` directory of this repo (`cargo build --release`) and make sure the resulting binary is on PATH.

## Installing this extension

1. Clone this repo.
2. In Zed: Command Palette → `zed: install dev extension` → select the cloned folder.
3. Open a `.sql` file — diagnostics should appear from `plsqllang-server`, routed through `plsqllang-proxy`.

## Limitations

- **Syntax checking only** — no semantic awareness of your actual database schema (tables, columns, packages aren't validated against a live connection).
- **Requires manually sourcing `server-all.jar`** as described above (see Prerequisites) — the upstream `plsqllang-server` repo currently can't be built from source standalone.
- **False positives on SQL\*Plus / SQLcl scripting syntax.** The parser targets PL/SQL blocks specifically and doesn't understand SQL\*Plus client directives. `plsqllang-proxy` filters out the known cases — `SET` commands (`SET SERVEROUTPUT ON`, `SET VERIFY OFF`, etc.), `DEFINE variable = value`, and `&substitution_variable` references — but any directive not yet in its filter list may still be flagged as a syntax error even though it runs fine in SQLcl or SQL\*Plus. If you hit one, it's a missing filter entry, not a real bug in your SQL.
- **Hover / go-to-definition may not resolve correctly.** `plsqllang-proxy` splits each document into per-statement virtual documents so that every statement gets checked (fixing the upstream single-statement limitation for diagnostics), but requests like hover or go-to-definition still reference positions in the real document — positions the underlying server never sees directly, since it only ever operates on the chunk-local virtual documents. These requests are forwarded unchanged and may not resolve correctly as a result.
- Single-maintainer upstream project with no releases — expect occasional rough edges in the underlying parser itself, independent of anything in this extension.
