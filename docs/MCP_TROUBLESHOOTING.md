# MCP Troubleshooting Guide

This guide helps diagnose and fix issues with tinyMem's Model Context Protocol (MCP) server integration.

## Quick Test

Before configuring your IDE, verify the MCP server works:

```bash
cd /path/to/your/project

# Send a test initialize message
echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}' | ./tinymem mcp
```

**Expected output:** A single line of JSON starting with `{"id":1,"jsonrpc":"2.0","result":{...`

**Problem signs:**
- No output within 2 seconds
- Error messages mixed with JSON
- Non-JSON text before the response

## Common Errors

### Error: "Request timed out" (MCP error -32001)

**Cause:** MCP client couldn't complete handshake with server

**Solutions:**

1. **Verify absolute path:**
   ```bash
   # Find your tinymem path
   which tinymem
   # or
   readlink -f tinymem  # Linux
   # or
   realpath tinymem     # macOS (if coreutils installed)
   ```

   Use the full path in your config:
   ```json
   {
     "mcpServers": {
       "tinymem": {
         "command": "/absolute/path/to/tinymem",
         "args": ["mcp"]
       }
     }
   }
   ```

2. **Check permissions:**
   ```bash
   ls -l /path/to/tinymem
   # Should show: -rwxr-xr-x (executable)
   chmod +x /path/to/tinymem  # If not executable
   ```

3. **Initialize database first:**
   ```bash
   cd /path/to/your/project
   ./tinymem health
   # Should create .tinyMem/ directory and store.sqlite3
   ```

4. **Test manually** (see Quick Test above)

### Error: "Client is not connected"

**Cause:** MCP server started but then disconnected/crashed

**Solutions:**

1. **Check logs:**
   ```bash
   tail -20 .tinyMem/logs/tinymem-$(date +%Y-%m-%d).log
   ```

   Look for:
   - Database errors
   - Filesystem permission errors
   - Configuration validation errors

2. **Verify database:**
   ```bash
   ls -la .tinyMem/
   # Should see: store.sqlite3 (not empty)
   file .tinyMem/store.sqlite3
   # Should say: SQLite 3.x database
   ```

3. **Check disk space:**
   ```bash
   df -h .
   # Ensure sufficient space for database
   ```

4. **Rebuild from source (FTS5 required):**
   ```bash
   go build -tags fts5 -o tinymem ./cmd/tinymem
   ```

   FTS5 support is mandatory; there is no build that omits the `fts5` tag.

### Error: "Tool not found: memory.query"

**Cause:** Using old tool naming (dots instead of underscores)

**Solution:** Tool names use underscores:
- ✅ `memory_query`
- ✅ `memory_recent`
- ✅ `memory_write`
- ✅ `memory_stats`
- ✅ `memory_health`
- ✅ `memory_doctor`

❌ Don't use: `memory.query`, `memory.recent`, etc.

### MCP Server Works Manually But Not in IDE

**Cause:** IDE using different working directory

**Solutions:**

1. **Set working directory in config:**

   For Cursor:
   ```json
   {
     "mcpServers": {
       "tinymem": {
         "command": "/absolute/path/to/tinymem",
         "args": ["mcp"],
         "cwd": "/path/to/your/project"
       }
     }
   }
   ```

2. **Use absolute path to tinymem binary**

3. **Check IDE logs** for actual error messages

## Debugging Steps

### Step 1: Verify Basic Functionality

```bash
cd /path/to/your/project
./tinymem health
./tinymem stats
./tinymem query "test"
```

All commands should work without errors.

### Step 2: Test MCP Protocol

```bash
# Start MCP server
./tinymem mcp &
MCP_PID=$!

# Send initialize
echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}' > /tmp/mcp_test

# Send to server
cat /tmp/mcp_test | ./tinymem mcp

# Kill test server
kill $MCP_PID
```

Should return valid JSON with no errors.

### Step 3: Check for stdio Interference

```bash
# Run MCP and capture all output
./tinymem mcp < /dev/null 2>&1 | head -1
```

**Should be:** Empty or valid JSON only
**Should NOT be:** Log messages, errors, or other text

### Step 4: Verify Logs Work

```bash
# Start MCP server briefly
echo '{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}' | ./tinymem mcp > /dev/null

# Check log file was created/updated
ls -lt .tinyMem/logs/ | head -2
```

Latest log should be recent (within last minute).

## Configuration Examples

### Claude Desktop (macOS)

File: `~/Library/Application Support/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "tinymem": {
      "command": "/usr/local/bin/tinymem",
      "args": ["mcp"]
    }
  }
}
```

### Cursor

File: `~/.cursor/config/mcp.json` or via Settings UI

```json
{
  "mcpServers": {
    "tinymem": {
      "command": "/Users/you/projects/tinymem",
      "args": ["mcp"],
      "cwd": "/Users/you/projects/yourproject"
    }
  }
}
```

### VS Code with MCP Extension

See extension documentation for configuration format.

## Still Having Issues?

1. **Check tinyMem version:**
   ```bash
   ./tinymem version
   # Should be v0.1.0 or later
   ```

2. **Run doctor diagnostics:**
   ```bash
   ./tinymem doctor
   ```

3. **Enable debug logging** (temporary):

   Edit `.tinyMem/config.toml`:
   ```toml
   log_level = "debug"
   ```

   Then check logs after attempting MCP connection:
   ```bash
   tail -50 .tinyMem/logs/tinymem-$(date +%Y-%m-%d).log
   ```

4. **Create a minimal test case:**
   ```bash
   # New directory, fresh start
   mkdir /tmp/tinymem-test
   cd /tmp/tinymem-test
   /path/to/tinymem health
   echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}' | /path/to/tinymem mcp
   ```

5. **Open an issue** with:
   - Output from `tinymem version`
   - Output from `tinymem doctor`
   - Your MCP configuration (sanitized)
   - Relevant log excerpts
   - OS and IDE version

## Technical Details

### MCP Protocol Requirements

- **Transport:** stdio (stdin/stdout)
- **Format:** JSON-RPC 2.0, one message per line
- **Critical:** No non-JSON output on stdout/stderr
- **Initialization:** Client sends `initialize`, server responds with capabilities

### tinyMem MCP Implementation

- **Logging:** Silent mode (file-only) when running as MCP server
- **Database:** Must exist before MCP server starts
- **Configuration:** Loaded from `.tinyMem/config.toml` if present
- **Project Scope:** All operations relative to working directory

### Protocol Flow

```
1. Client → initialize request
2. Server → initialize response (with capabilities and tools)
3. Client → tools/list request
4. Server → tools/list response (6 tools)
5. Client → tools/call requests (as needed)
6. Server → tool call responses (with content)
```

Any deviation from this flow indicates a problem.

## Prevention

To avoid MCP issues in the future:

1. **Use absolute paths** in all configurations
2. **Test locally first** before configuring IDE
3. **Run `tinymem health`** after pulling updates
4. **Check logs** when behavior seems odd
5. **Keep working directory consistent** (project root)
