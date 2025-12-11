# ModelPreds 超时配置说明

## 问题
Rust MCP Server 在等待 Python agent 响应时出现超时：
```
Model prediction TIMEOUT - skipping this tick
```

## 超时位置
超时是在 `extrema_infra` 库中定义的，不在当前代码库中。

## 解决方案

### 方案 1: 优化 Python 响应速度（已实施）
- ✅ 减少默认 `max_tokens` 从 1000 到 300
- ✅ 简化 prompt 格式，减少 LLM 需要处理的文本量
- ✅ 添加性能日志，监控响应时间

### 方案 2: 检查环境变量
`extrema_infra` 库可能支持通过环境变量配置超时时间，尝试设置：
```bash
# Windows PowerShell
$env:MODEL_PREDS_TIMEOUT_SEC="60"
# 或
$env:EXTREMA_MODEL_TIMEOUT="60"

# 然后运行 Rust server
cargo run
```

### 方案 3: 修改 extrema_infra 库
如果超时是硬编码的，需要：
1. Fork `extrema_infra` 仓库
2. 找到超时设置的位置（通常在 `arch/task_execution/register_alt.rs` 或类似文件）
3. 增加超时时间（例如从 10 秒增加到 60 秒）
4. 更新 `Cargo.toml` 使用你的 fork

### 方案 4: 联系 extrema_infra 维护者
如果无法修改库，可以：
- 在 GitHub issue 中请求添加超时配置选项
- 或者请求增加默认超时时间

## 当前优化
- Python prompt 已简化，减少 LLM 处理时间
- 默认 max_tokens 已减少到 300
- 添加了详细的性能日志

## 建议
1. 先尝试方案 2（环境变量）
2. 如果不行，考虑方案 3（修改库）
3. 监控 Python 日志中的 latency，确保响应时间在合理范围内

