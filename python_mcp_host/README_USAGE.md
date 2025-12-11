# 交易 Agent 使用指南

## 快速开始

### 1. 基本启动（交互式定义交易风格）

```bash
cd python_mcp_host
uv run mcp_host --port 5001
```

启动后会提示你输入交易风格，你可以：
- 直接输入你的交易风格描述
- 直接回车使用默认的稳健型风格

### 2. 从文件加载交易风格

#### 方式 A: 使用 JSON 文件

创建 `my_trading_style.json`:
```json
{
  "trading_style": "稳健型交易风格：\n- 优先控制风险，单次交易风险不超过总资金的 20%\n- 仓位管理：正常市场条件下仓位控制在 30-50%\n- 注重止损，设置合理的止损点位"
}
```

然后运行：
```bash
uv run mcp_host --port 5001 --trading-style-file my_trading_style.json
```

#### 方式 B: 使用文本文件

创建 `my_trading_style.txt`:
```
稳健型交易风格：
- 优先控制风险，单次交易风险不超过总资金的 20%
- 仓位管理：正常市场条件下仓位控制在 30-50%
- 注重止损，设置合理的止损点位
- 不追求短期暴利，注重长期稳定收益
```

然后运行：
```bash
uv run mcp_host --port 5001 --trading-style-file my_trading_style.txt
```

### 3. 直接在命令行输入交易风格

```bash
uv run mcp_host --port 5001 --trading-style "激进型：追求高收益，仓位可以到70-80%，快速响应市场变化"
```

## 完整参数说明

```bash
uv run mcp_host [选项]

选项：
  --port PORT               ZMQ 服务端口（必需，或设置 INFER_PORT 环境变量）
  --config PATH             模型配置文件路径（默认: model_config.json）
  --trading-style TEXT      直接输入交易风格文本
  --trading-style-file PATH 从文件加载交易风格（JSON 或文本文件）
  --prompt TEXT             测试模式：单次运行并显示结果（不启动服务器）
  --model-id ID             模型 ID（仅在 --prompt 模式下使用，默认: gemini_one）
```

## 使用示例

### 示例 1: 使用预定义的稳健风格

```bash
# 使用示例文件
uv run mcp_host --port 5001 --trading-style-file trading_style_example.json
```

### 示例 2: 自定义激进风格

```bash
uv run mcp_host --port 5001 --trading-style "激进型交易风格：
- 追求高收益，可以承担较大风险
- 仓位管理：正常市场条件下仓位可以到 70-80%
- 快速响应市场变化，抓住短期机会
- 基于 Z-Score 特征，当特征显著偏离（|z| > 1.5）时，积极操作"
```

### 示例 3: 测试模式（单次运行）

```bash
# 测试 prompt，不启动服务器
uv run mcp_host --port 5001 --prompt "当前市场如何？"
```

## 交易风格示例

参考 `trading_style_examples.txt` 文件，包含以下风格：

1. **稳健型** - 优先控制风险，仓位较小
2. **激进型** - 追求高收益，可以承担较大风险
3. **趋势跟踪型** - 跟随市场趋势，顺势而为
4. **均值回归型** - 在价格偏离均值时反向操作
5. **平衡型** - 在风险和收益之间寻求平衡

## 工作流程

1. **启动 Agent**
   ```bash
   uv run mcp_host --port 5001 --trading-style-file my_style.json
   ```

2. **Agent 等待 Rust MCP Server 发送数据**
   - Agent 会持续运行，等待接收市场数据
   - 当 Rust 发送 AltTensor 数据时，Agent 会自动：
     - 将数据转换为 prompt
     - 调用 LLM 进行交易决策
     - 解析响应并提取交易指令
     - 返回给 Rust MCP Server

3. **查看日志**
   - Agent 会输出详细的日志信息
   - 包括：接收的数据、LLM 响应、解析的交易指令等

## 注意事项

1. **交易风格的重要性**
   - 交易风格会影响 Agent 的所有决策
   - 建议根据你的风险偏好仔细定义
   - 可以在运行前测试不同的风格

2. **权重范围**
   - 仓位权重范围是 **-1 到 1**
   - `1.0` = 满仓做多
   - `0.0` = 空仓
   - `-1.0` = 满仓做空

3. **配置文件**
   - 确保 `model_config.json` 中配置了正确的 API Key
   - 确保端口号与 Rust MCP Server 一致（默认 5001）

## 故障排查

### 问题：提示 API key 错误
**解决**：检查 `model_config.json` 中的 `api_key` 字段，或设置 `GEMINI_API_KEY` 环境变量

### 问题：端口被占用
**解决**：使用 `--port` 参数指定其他端口，或确保 Rust MCP Server 使用相同端口

### 问题：交易风格文件读取失败
**解决**：检查文件路径是否正确，文件编码是否为 UTF-8

## 完整示例

```bash
# 1. 创建交易风格文件
cat > my_style.txt << EOF
平衡型交易风格：
- 在风险和收益之间寻求平衡
- 仓位管理：正常市场条件下仓位控制在 40-60%
- 结合趋势和均值回归策略
- 基于 Z-Score 特征，当特征显著偏离（|z| > 1.5）时，适度操作
EOF

# 2. 启动 Agent
uv run mcp_host --port 5001 --trading-style-file my_style.txt

# 3. Agent 开始运行，等待 Rust 发送数据
# 日志会显示：
# [Agent] Starting automated trading agent (ZMQ server mode)...
# [Agent] Trading style loaded successfully
# [Agent] Waiting for Rust MCP server to send AltTensor data...
```

