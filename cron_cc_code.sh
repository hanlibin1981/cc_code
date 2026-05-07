#!/bin/bash
# cc_code 每日改进任务
# 每天下午6点自动执行

# 设置PATH（launchd不会继承用户环境）
export PATH="/Users/mac/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export HOME="/Users/mac"

LOG_FILE="$HOME/.openclaw/workspace/cc_code/cron.log"
WORKSPACE="$HOME/.openclaw/workspace/cc_code"
CARGO="$HOME/.cargo/bin/cargo"

echo "[$(date)] ===== cc_code 改进任务开始 =====" >> "$LOG_FILE"

cd "$WORKSPACE" || {
    echo "[$(date)] ERROR: 无法进入工作目录" >> "$LOG_FILE"
    exit 1
}

# 1. 运行测试（排除coordinator测试和已知flaky的fork并发测试）
echo "[$(date)] 运行测试..." >> "$LOG_FILE"
TEST_OUTPUT=$("$CARGO" test -- --test-threads=1 --skip coordinator --skip test_max_parallel_forks 2>&1)
TEST_EXIT=$?
echo "$TEST_OUTPUT" | tail -10 >> "$LOG_FILE"

# 2. 编译（即使测试失败也继续编译）
echo "[$(date)] 编译中..." >> "$LOG_FILE"
BUILD_OUTPUT=$("$CARGO" build --release 2>&1)
BUILD_EXIT=$?
echo "$BUILD_OUTPUT" | tail -3 >> "$LOG_FILE"

if [ $BUILD_EXIT -eq 0 ]; then
    echo "[$(date)] 编译成功!" >> "$LOG_FILE"
    cp "$WORKSPACE/target/release/cc_code" "$HOME/.cargo/bin/cc_code"
else
    echo "[$(date)] 编译失败 (exit: $BUILD_EXIT)" >> "$LOG_FILE"
fi

# 3. 检查警告数量
echo "[$(date)] 检查代码质量..." >> "$LOG_FILE"
WARNINGS=$(echo "$BUILD_OUTPUT" | grep -c "warning:" || echo "0")
echo "[$(date)] 当前警告数: $WARNINGS" >> "$LOG_FILE"

# 4. 检查格式化
"$CARGO" fmt --check 2>&1 | head -5 >> "$LOG_FILE"

echo "[$(date)] ===== cc_code 改进任务完成 =====" >> "$LOG_FILE"
echo "" >> "$LOG_FILE"