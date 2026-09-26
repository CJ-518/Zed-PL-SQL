[English](README.md) | [Español](README.es.md)

# Zed-PL-SQL

Zed extension wiring [plsqllang-server](https://github.com/EwanDubashinski/plsqllang-server) (a PL/SQL LSP for syntax checking) into Zed as a language server for SQL files.

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

## Installing this extension

1. Clone this repo.
2. In Zed: Command Palette → `zed: install dev extension` → select the cloned folder.
3. Open a `.sql` file — diagnostics should appear from `plsqllang-server`.

## Limitations

- **Syntax checking only** — no semantic awareness of your actual database schema (tables, columns, packages aren't validated against a live connection).
- **Requires manually sourcing `server-all.jar`** as described above (see Prerequisites) — the upstream `plsqllang-server` repo currently can't be built from source standalone.
- **False positives on SQL\*Plus / SQLcl scripting syntax.** The parser targets PL/SQL blocks specifically and doesn't understand SQL\*Plus client directives. Expect it to flag valid lines like:
  - `SET SERVEROUTPUT ON`, `SET VERIFY OFF`, and other `SET` commands
  - `DEFINE variable = value`
  - `&substitution_variable` references

  as syntax errors even though they run fine in SQLcl or SQL\*Plus. These are false positives from parser scope, not real bugs in your SQL — disregard diagnostics on lines using this kind of client-side syntax.
- Single-maintainer upstream project with no releases — expect occasional rough edges in the underlying parser itself, independent of anything in this extension.
