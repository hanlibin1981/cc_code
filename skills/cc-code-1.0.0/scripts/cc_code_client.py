#!/usr/bin/env python3
"""
cc_code Client - OpenClaw Skill
调用 cc_code MCP 服务器进行编程推理，工具执行由本脚本完成。
"""

import subprocess
import json
import sys
import os
import re
import glob as glob_module
import time
import fcntl
import select
from typing import Optional

CC_CODE_BIN = os.path.expanduser("~/.openclaw/workspace/cc_code/target/debug/cc_code")
MAX_ITERATIONS = 20


class ToolExecutor:
    def execute(self, tool_name: str, arguments: dict) -> tuple[str, bool]:
        try:
            if tool_name == "read_file":
                return self._read_file(arguments)
            elif tool_name == "write_file":
                return self._write_file(arguments)
            elif tool_name == "edit_file":
                return self._edit_file(arguments)
            elif tool_name == "bash":
                return self._bash(arguments)
            elif tool_name == "glob":
                return self._glob(arguments)
            elif tool_name == "grep":
                return self._grep(arguments)
            else:
                return f"未知工具: {tool_name}", True
        except Exception as e:
            return str(e), True
    
    def _read_file(self, args: dict) -> tuple[str, bool]:
        path = args.get("path", "")
        if not path:
            return "缺少 path 参数", True
        try:
            with open(path, "r", encoding="utf-8") as f:
                content = f.read()
            return f"文件: {path}\n---\n{content[:5000]}", False
        except Exception as e:
            return f"读取失败: {e}", True
    
    def _write_file(self, args: dict) -> tuple[str, bool]:
        path, content = args.get("path", ""), args.get("content", "")
        if not path:
            return "缺少 path 参数", True
        try:
            os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
            with open(path, "w", encoding="utf-8") as f:
                f.write(content)
            return f"已写入: {path} ({len(content)} 字符)", False
        except Exception as e:
            return f"写入失败: {e}", True
    
    def _edit_file(self, args: dict) -> tuple[str, bool]:
        path, old_text, new_text = args.get("path", ""), args.get("old_text", ""), args.get("new_text", "")
        if not path or not old_text:
            return "缺少 path 或 old_text 参数", True
        try:
            with open(path, "r", encoding="utf-8") as f:
                content = f.read()
            if old_text not in content:
                return f"未找到: {old_text[:50]}", True
            new_content = content.replace(old_text, new_text, 1)
            with open(path, "w", encoding="utf-8") as f:
                f.write(new_content)
            return f"已修改: {path}", False
        except Exception as e:
            return f"修改失败: {e}", True
    
    def _bash(self, args: dict) -> tuple[str, bool]:
        cmd = args.get("command", "")
        if not cmd:
            return "缺少 command 参数", True
        dangerous = ["rm -rf /", "dd if=/dev/zero of=/dev/sd", ":(){:|:&};:", "curl | sh", "wget -O- | sh"]
        for d in dangerous:
            if d in cmd.replace(" ", ""):
                return f"危险命令已拒绝", True
        try:
            result = subprocess.run(cmd, shell=True, capture_output=True, text=True,
                                  timeout=min(args.get("timeout", 30), 60))
            output = result.stdout + result.stderr
            if result.returncode != 0 and not output.strip():
                output = f"(退出码: {result.returncode})"
            return output[:10000], False
        except subprocess.TimeoutExpired:
            return "命令超时", True
        except Exception as e:
            return f"执行失败: {e}", True
    
    def _glob(self, args: dict) -> tuple[str, bool]:
        pattern, cwd = args.get("pattern", "**/*.rs"), args.get("cwd", ".")
        if not pattern:
            return "缺少 pattern 参数", True
        try:
            files = glob_module.glob(pattern, root_dir=cwd, recursive=True)
            if not files:
                return f"未找到: {pattern}", False
            return "找到文件:\n" + "\n".join(f"  {f}" for f in files[:100]), False
        except Exception as e:
            return f"glob 失败: {e}", True
    
    def _grep(self, args: dict) -> tuple[str, bool]:
        pattern, paths = args.get("pattern", ""), args.get("paths", ["."])
        if not pattern:
            return "缺少 pattern 参数", True
        try:
            results = []
            for path in paths:
                if os.path.isfile(path):
                    paths_to_search = [path]
                elif os.path.isdir(path):
                    paths_to_search = [os.path.join(r, f) for r, _, fs in os.walk(path) for f in fs
                                     if f.endswith(('.rs', '.py', '.js', '.ts', '.md', '.txt'))]
                else:
                    continue
                for filepath in paths_to_search[:50]:
                    try:
                        with open(filepath, "r", encoding="utf-8", errors="ignore") as f:
                            for i, line in enumerate(f, 1):
                                if pattern in line:
                                    results.append(f"{filepath}:{i}: {line.rstrip()}")
                                    if len(results) >= 50:
                                        break
                    except:
                        pass
                if len(results) >= 50:
                    break
            if not results:
                return f"未找到: {pattern}", False
            return "匹配结果:\n" + "\n".join(f"  {r}" for r in results), False
        except Exception as e:
            return f"grep 失败: {e}", True


class CcCodeClient:
    def __init__(self, cwd: str = "."):
        self.cwd = cwd
        self.session_id: Optional[str] = None
        self.process: Optional[subprocess.Popen] = None
        self.tool_executor = ToolExecutor()
        self._req_id = 1
    
    def start(self):
        if not os.path.exists(CC_CODE_BIN):
            raise RuntimeError(f"cc_code 未找到: {CC_CODE_BIN}")
        
        env = os.environ.copy()
        api_key = env.get("MINIMAX_API_KEY", "")
        _ = api_key
        
        self.process = subprocess.Popen(
            [CC_CODE_BIN],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0
        )
        
        # 设置 stderr 为非阻塞
        flags = fcntl.fcntl(self.process.stderr, fcntl.F_GETFL)
        fcntl.fcntl(self.process.stderr, fcntl.F_SETFL, flags | os.O_NONBLOCK)
        
        self._drain_stderr(timeout=1.0)
        
        resp = self._call("initialize", {
            "protocol_version": {"major": 1, "minor": 0},
            "capabilities": {},
            "client_info": {"name": "cc-client", "version": "1.0"}
        })
        if not resp:
            raise RuntimeError("初始化失败")
        
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        self._drain_stderr()
    
    def _drain_stderr(self, timeout: float = 0.5):
        if not self.process:
            return
        deadline = time.time() + timeout
        while time.time() < deadline:
            ready, _, _ = select.select([self.process.stderr], [], [], 0.1)
            if ready:
                try:
                    chunk = os.read(self.process.stderr.fileno(), 4096)
                    if not chunk:
                        break
                except OSError:
                    break
    
    def _send(self, req: dict):
        if not self.process:
            return
        line = json.dumps(req) + "\n"
        os.write(self.process.stdin.fileno(), line.encode('utf-8'))
    
    def _call(self, method: str, params: dict = None) -> Optional[dict]:
        req_id = self._req_id
        self._req_id += 1
        req = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params:
            req["params"] = params
        
        self._send(req)
        
        deadline = time.time() + 300  # 5min for complex tasks
        buf = b""
        last_read_time = time.time()
        
        while time.time() < deadline:
            self._drain_stderr(timeout=0.05)
            # Always try to read if we haven't read in a while (avoid starvation)
            if time.time() - last_read_time > 0.5:
                try:
                    chunk = os.read(self.process.stdout.fileno(), 16384)
                    if chunk:
                        buf += chunk
                        last_read_time = time.time()
                    elif buf:
                        # EOF on stdout but still have buffered data
                        pass
                    else:
                        # No data and no buffer, wait a bit
                        time.sleep(0.05)
                        continue
                except OSError:
                    time.sleep(0.05)
                    continue
            
            # Try to parse from buffer
            text = buf.decode('utf-8', errors='replace')
            text_stripped = text.lstrip()
            if text_stripped.startswith('{'):
                try:
                    return json.loads(text_stripped)
                except json.JSONDecodeError:
                    pass
            
            # Try line-by-line
            for line in text.split('\n'):
                line = line.strip()
                if line.startswith('{'):
                    try:
                        return json.loads(line)
                    except json.JSONDecodeError:
                        pass
            
            # If buffer is growing but incomplete, keep reading
            if len(buf) < 1024 * 1024:  # 1MB max
                time.sleep(0.05)
                continue
            break
        return None
    
    def create_session(self) -> str:
        resp = self._call("tools/call", {
            "name": "cc_start_session",
            "arguments": {"cwd": self.cwd}
        })
        if not resp or "result" not in resp:
            raise RuntimeError(f"创建会话失败: {resp}")
        text = resp["result"]["content"][0]["text"]
        self.session_id = text.split(": ")[1].split()[0]
        return self.session_id
    
    def send_message(self, message: str, tool_results: list = None) -> str:
        args = {"session_id": self.session_id, "message": message}
        if tool_results:
            args["tool_results"] = tool_results
        resp = self._call("tools/call", {
            "name": "cc_send_message",
            "arguments": args
        })
        if not resp or "result" not in resp:
            return f"调用失败: {resp}"
        return resp["result"]["content"][0]["text"]
    
    def stop(self):
        if self.process:
            try:
                self.process.stdin.close()
                self.process.wait(timeout=3)
            except:
                self.process.kill()
    
    def parse_tool_calls(self, text: str) -> list:
        """解析 [TOOL_CALL:{...}] 格式，支持跨行和嵌套 JSON 对象"""
        tool_calls = []
        start_tag = '[TOOL_CALL:'
        idx = 0
        while True:
            start = text.find(start_tag, idx)
            if start == -1:
                break
            brace_start = text.find('{', start + len(start_tag))
            if brace_start == -1:
                break
            # 平衡括号，找匹配的 }
            depth = 0
            i = brace_start
            while i < len(text):
                c = text[i]
                if c == '{':
                    depth += 1
                elif c == '}':
                    depth -= 1
                    if depth == 0:
                        json_str = text[brace_start:i+1]
                        try:
                            tc = json.loads(json_str)
                            if isinstance(tc, dict) and "name" in tc:
                                tool_calls.append(tc)
                        except json.JSONDecodeError:
                            pass
                        break
                i += 1
            idx = brace_start + 1
        return tool_calls
    
    def run(self, task: str) -> str:
        self.start()
        try:
            self.create_session()
            tool_results = None
            for i in range(MAX_ITERATIONS):
                # API 调用失败重试（指数退避，最多 3 次）
                for retry in range(3):
                    response = self.send_message(task, tool_results)
                    # 检测 API 错误
                    if "处理消息失败" in response or "API error" in response or "Model API error" in response:
                        wait = (retry + 1) * 3
                        print(f"⚠️ API 错误，重试 ({retry+1}/3)，等待 {wait}s: {response[:80]}", file=sys.stderr)
                        time.sleep(wait)
                        continue
                    break
                else:
                    # 3 次重试都失败
                    return response

                tool_calls = self.parse_tool_calls(response)
                if not tool_calls:
                    return response
                tool_results = []
                for tc in tool_calls:
                    tool_name = tc.get("name", "")
                    arguments = tc.get("arguments", {})
                    print(f"🔧 [{i+1}] {tool_name}", file=sys.stderr)
                    output, is_error = self.tool_executor.execute(tool_name, arguments)
                    tool_results.append({"tool": tool_name, "result": output, "is_error": is_error})
                task = "继续"
        finally:
            self.stop()
        return "达到最大迭代次数"


def main():
    if len(sys.argv) < 2:
        print("用法: cc_code_client.py <任务> [目录]", file=sys.stderr)
        sys.exit(1)
    task = sys.argv[1]
    cwd = sys.argv[2] if len(sys.argv) > 2 else os.getcwd()
    print(f"📋 任务: {task}", file=sys.stderr)
    try:
        client = CcCodeClient(cwd)
        result = client.run(task)
        print(result)
    except Exception as e:
        import traceback
        traceback.print_exc(file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
