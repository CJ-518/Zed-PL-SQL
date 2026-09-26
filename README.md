'# zed-plsqllang

Zed extension wiring plsqllang-server (a PL/SQL LSP for syntax checking) into Zed as a language server for SQL files.

## Prerequisites

You need a plsqllang-server binary on your PATH. The easiest source is the
server-all.jar bundled inside the plsqllang-client VS Code extension .vsix
file (under extension/server/server-all.jar) - building the plsqllang-server
repo from source directly currently fails, since it depends on a parser
module that is not publicly published.

Requires a JDK (1.8+).

### Windows
Create plsqllang-server.bat somewhere on your PATH:
@echo off
java -jar "C:\path\to\server-all.jar" %*

### macOS / Linux
Create an executable plsqllang-server shell script somewhere on your PATH:
#!/usr/bin/env bash
exec java -jar /path/to/server-all.jar "$@"

## Installing this extension

1. Clone this repo.
2. In Zed: Command Palette -> zed: install dev extension -> select the cloned folder.
3. Open a .sql file - diagnostics should appear from plsqllang-server.

## Limitations

- Syntax checking only - no semantic awareness of your actual database schema.
- Requires manually sourcing server-all.jar as described above (see Prerequisites).' | Out-File -Encoding ASCII README.md
