---
name: claude-code-mcp
description: Delegate coding tasks to Claude Code via MCP server. Use when user asks to generate code, fix bugs, refactor, write tests, code review, or explain code using Claude Code MCP tools.
metadata: {"openclaw": {"emoji": "🤖", "requires": {"bins": ["mcporter"]}}}
---

# Claude Code MCP

Use the Claude Code MCP server via mcporter to handle coding tasks.

## MCP Server Command

```bash
bash -c 'PYTHONPATH=/Users/mac/openclaw-projects/claude-code-mcp-server/src /Users/mac/openclaw-projects/claude-code-mcp-server/.venv/bin/python3 -m claude_code_mcp.server'
```

## Available Tools

Call via: `mcporter call --stdio "<cmd>" <tool> <params> --output json`

| Tool | When to use |
|------|-------------|
| `generate_code` | Generate new code from description |
| `fix_bugs` | Fix bugs given error message |
| `refactor_code` | Refactor for quality/performance |
| `write_tests` | Generate unit tests |
| `code_review` | Review code quality |
| `explain_code` | Explain what code does |
| `complete_task` | Complex multi-step coding task |

## Examples

**Generate code:**
```bash
mcporter call --stdio "bash -c 'PYTHONPATH=/Users/mac/openclaw-projects/claude-code-mcp-server/src /Users/mac/openclaw-projects/claude-code-mcp-server/.venv/bin/python3 -m claude_code_mcp.server'" generate_code description="Hello world Python script" language=python --output json
```

**Explain code:**
```bash
mcporter call --stdio "bash -c 'PYTHONPATH=/Users/mac/openclaw-projects/claude-code-mcp-server/src /Users/mac/openclaw-projects/claude-code-mcp-server/.venv/bin/python3 -m claude_code_mcp.server'" explain_code code="print('hello')" language=python --output json
```

**Fix bugs:**
```bash
mcporter call --stdio "bash -c 'PYTHONPATH=/Users/mac/openclaw-projects/claude-code-mcp-server/src /Users/mac/openclaw-projects/claude-code-mcp-server/.venv/bin/python3 -m claude_code_mcp.server'" fix_bugs code="import os print('hello')" error="SyntaxError" language=python --output json
```

## Notes

- Timeout: 300s for most calls
- Claude Code must be signed in: `claude auth status`
- The MCP server must be running (starts on first call)
