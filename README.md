# MCP Trading Agent - 交易 Agent 系统

[English](#english) | [中文](#chinese)

---

## <a name="chinese"></a>中文使用指南

### 📖 项目简介

这是一个基于 MCP (Model Context Protocol) 的自动化交易系统，由 Rust MCP Server 和 Python LLM Agent 组成。系统能够：
- 实时接收市场数据（价格、持仓量等）
- 使用 LLM (Gemini) 进行智能交易决策
- 自动执行仓位调整
- 支持做多和做空（-1 到 1 的仓位权重）

### 🏗️ 系统架构

```
Rust MCP Server (数据采集)
    ↓ (发送 AltTensor via ZMQ)
Python Trading Agent (LLM 决策)
    ↓ (解析响应，提取交易指令)
Rust MCP Mediator (执行交易)
```

### 📦 安装步骤

#### 1. 安装 Rust 环境
```bash
# 安装 Rust (如果还没有)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### 2. 安装 Python 环境 (使用 uv)
```bash
# 安装 uv (如果还没有)
pip install uv

# 进入 Python 项目目录
cd python_mcp_host

# 安装依赖
uv sync
```

#### 3. 配置 API Key

编辑 `python_mcp_host/model_config.json`:

**Gemini 配置示例：**
```json
[
  {
    "port": 5001,
    "model_id": "gemini_one",
    "account_id": "okx_test",
    "llm_provider": "gemini",
    "api_key": "YOUR_GEMINI_API_KEY",
    "model_name": "gemini-2.5-flash-lite"
  }
]
```

**DeepSeek 配置示例：**
```json
[
  {
    "port": 5002,
    "model_id": "deepseek_one",
    "account_id": "okx_test",
    "llm_provider": "deepseek",
    "api_key": "YOUR_DEEPSEEK_API_KEY",
    "model_name": "deepseek-chat",
    "base_url": "https://api.deepseek.com"
  }
]
```

**同时使用多个 LLM：**
```json
[
  {
    "port": 5001,
    "model_id": "gemini_one",
    "account_id": "okx_test",
    "llm_provider": "gemini",
    "api_key": "YOUR_GEMINI_API_KEY",
    "model_name": "gemini-2.5-flash-lite"
  },
  {
    "port": 5002,
    "model_id": "deepseek_one",
    "account_id": "okx_test",
    "llm_provider": "deepseek",
    "api_key": "YOUR_DEEPSEEK_API_KEY",
    "model_name": "deepseek-chat"
  }
]
```

或者设置环境变量：
```bash
# Gemini
export GEMINI_API_KEY="YOUR_GEMINI_API_KEY"

# DeepSeek
export DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY"
```

#### 4. 配置交易账户

编辑 `rust_mcp_server/account_config.json`:
```json
[
  {
    "account_id": "okx_test",
    "exchange": "okx",
    "api_key": "YOUR_OKX_API_KEY",
    "api_secret": "YOUR_OKX_SECRET",
    "passphrase": "YOUR_PASSPHRASE"
  }
]
```

### 🚀 快速开始

#### 步骤 1: 启动 Python Trading Agent

```bash
cd python_mcp_host

# 方式 1: 交互式定义交易风格（推荐首次使用）
uv run mcp_host --port 5001

# 方式 2: 使用预定义的交易风格文件
uv run mcp_host --port 5001 --trading-style-file trading_style_example.json

# 方式 3: 直接在命令行输入交易风格
uv run mcp_host --port 5001 --trading-style "稳健型：优先控制风险，仓位30-50%"
```

**交易风格示例文件：**
- `trading_style_example.json` - JSON 格式示例
- `trading_style_examples.txt` - 多种风格示例（稳健型、激进型、趋势跟踪型等）

#### 步骤 2: 启动 Rust MCP Server

```bash
cd rust_mcp_server
cargo run
```

### 📝 详细使用方法

#### Python Agent 参数说明

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

#### 交易风格定义

交易风格会影响 Agent 的所有交易决策。你可以：

1. **使用示例文件**
   ```bash
   uv run mcp_host --port 5001 --trading-style-file trading_style_example.json
   ```

2. **自定义交易风格文件**
   
   创建 `my_style.txt`:
   ```
   稳健型交易风格：
   - 优先控制风险，单次交易风险不超过总资金的 20%
   - 仓位管理：正常市场条件下仓位控制在 30-50%
   - 注重止损，设置合理的止损点位
   - 基于 Z-Score 特征，当特征显著偏离（|z| > 2）时，谨慎操作
   ```
   
   然后运行：
   ```bash
   uv run mcp_host --port 5001 --trading-style-file my_style.txt
   ```

3. **交互式输入**
   ```bash
   uv run mcp_host --port 5001
   # 程序会提示你输入交易风格
   ```

#### 测试模式

测试 LLM 响应（不启动服务器）：
```bash
uv run mcp_host --port 5001 --prompt "当前市场如何？建议仓位多少？"
```

### 📊 运行日志示例

启动后，你会看到类似以下的日志：

```
[Agent] 🚀 Starting server on port 5001
[Agent] ✅ Loaded 1 model(s)
[Agent] 🔌 ZMQ bound to tcp://127.0.0.1:5001
[Agent] ⏳ Waiting for data from Rust MCP server...
[Agent] 📨 Received request | model_id=gemini_one
[Agent] 📊 Received | price=0.12345 | pos=0.5 | features=10
[Agent] 📝 Generated prompt (1234 chars)
[Agent] 🤖 Calling LLM...
[Agent] 💬 LLM Response: 根据当前市场数据，我建议...
[Agent] ✅ Decision | cmd=adjust_position | inst=DOGE_USDT_PERP | target_pos=0.6 | latency=3000ms
```

### ⚙️ 配置说明

#### 仓位权重范围

- **范围**: -1 到 1
- **1.0**: 满仓做多
- **0.0**: 空仓
- **-1.0**: 满仓做空
- **0.5**: 50% 做多
- **-0.5**: 50% 做空

#### 交易风格类型

参考 `trading_style_examples.txt`，包含：
- **稳健型**: 优先控制风险，仓位较小
- **激进型**: 追求高收益，可以承担较大风险
- **趋势跟踪型**: 跟随市场趋势，顺势而为
- **均值回归型**: 在价格偏离均值时反向操作
- **平衡型**: 在风险和收益之间寻求平衡

### 🔧 故障排查

#### 问题 1: API Key 错误
```
ValueError: API key is required
```
**解决**: 
- Gemini: 检查 `model_config.json` 中的 `api_key` 字段，或设置 `GEMINI_API_KEY` 环境变量
- DeepSeek: 检查 `model_config.json` 中的 `api_key` 字段，或设置 `DEEPSEEK_API_KEY` 环境变量

#### 问题 1.1: DeepSeek 导入错误
```
ImportError: OpenAI package is required for DeepSeek
```
**解决**: 安装 OpenAI 包：`pip install openai` 或 `uv sync`（会自动安装依赖）

#### 问题 2: 端口被占用
```
Error: Address already in use
```
**解决**: 使用 `--port` 参数指定其他端口，或确保 Rust MCP Server 使用相同端口

#### 问题 3: 超时错误
```
Model prediction TIMEOUT - skipping this tick
```
**解决**: 
- Python Agent 已优化响应速度（减少 token 数量）
- 如果仍然超时，可能需要修改 `extrema_infra` 库的超时设置
- 参考 `rust_mcp_server/TIMEOUT_CONFIG.md`

#### 问题 4: 价格数据未找到
```
Price for DOGE_USDT_PERP not available yet
```
**解决**: 等待几秒钟让 WebSocket 价格数据到达，这是正常的

### 📚 更多信息

- Python Agent 详细文档: `python_mcp_host/README_USAGE.md`
- 超时配置说明: `rust_mcp_server/TIMEOUT_CONFIG.md`
- 交易风格示例: `python_mcp_host/trading_style_examples.txt`

---

## <a name="english"></a>English Usage Guide

### 📖 Project Overview

This is an automated trading system based on MCP (Model Context Protocol), consisting of a Rust MCP Server and a Python LLM Agent. The system can:
- Receive real-time market data (prices, open interest, etc.)
- Use LLM (Gemini / DeepSeek) for intelligent trading decisions
- Automatically execute position adjustments
- Support both long and short positions (-1 to 1 position weights)
- Support multiple LLM providers running simultaneously

### 🏗️ System Architecture

```
Rust MCP Server (Data Collection)
    ↓ (Send AltTensor via ZMQ)
Python Trading Agent (LLM Decision)
    ↓ (Parse response, extract trading commands)
Rust MCP Mediator (Execute Trading)
```

### 📦 Installation

#### 1. Install Rust Environment
```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### 2. Install Python Environment (using uv)
```bash
# Install uv (if not already installed)
pip install uv

# Navigate to Python project directory
cd python_mcp_host

# Install dependencies
uv sync
```

#### 3. Configure API Key

Edit `python_mcp_host/model_config.json`:

**Gemini Configuration Example:**
```json
[
  {
    "port": 5001,
    "model_id": "gemini_one",
    "account_id": "okx_test",
    "llm_provider": "gemini",
    "api_key": "YOUR_GEMINI_API_KEY",
    "model_name": "gemini-2.5-flash-lite"
  }
]
```

**DeepSeek Configuration Example:**
```json
[
  {
    "port": 5002,
    "model_id": "deepseek_one",
    "account_id": "okx_test",
    "llm_provider": "deepseek",
    "api_key": "YOUR_DEEPSEEK_API_KEY",
    "model_name": "deepseek-chat",
    "base_url": "https://api.deepseek.com"
  }
]
```

**Using Multiple LLMs:**
```json
[
  {
    "port": 5001,
    "model_id": "gemini_one",
    "account_id": "okx_test",
    "llm_provider": "gemini",
    "api_key": "YOUR_GEMINI_API_KEY",
    "model_name": "gemini-2.5-flash-lite"
  },
  {
    "port": 5002,
    "model_id": "deepseek_one",
    "account_id": "okx_test",
    "llm_provider": "deepseek",
    "api_key": "YOUR_DEEPSEEK_API_KEY",
    "model_name": "deepseek-chat"
  }
]
```

Or set environment variables:
```bash
# Gemini
export GEMINI_API_KEY="YOUR_GEMINI_API_KEY"

# DeepSeek
export DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY"
```

#### 4. Configure Trading Account

Edit `rust_mcp_server/account_config.json`:
```json
[
  {
    "account_id": "okx_test",
    "exchange": "okx",
    "api_key": "YOUR_OKX_API_KEY",
    "api_secret": "YOUR_OKX_SECRET",
    "passphrase": "YOUR_PASSPHRASE"
  }
]
```

### 🚀 Quick Start

#### Step 1: Start Python Trading Agent

```bash
cd python_mcp_host

# Method 1: Interactive trading style definition (recommended for first use)
uv run mcp_host --port 5001

# Method 2: Use predefined trading style file
uv run mcp_host --port 5001 --trading-style-file trading_style_example.json

# Method 3: Enter trading style directly in command line
uv run mcp_host --port 5001 --trading-style "Conservative: prioritize risk control, position 30-50%"
```

**Trading Style Example Files:**
- `trading_style_example.json` - JSON format example
- `trading_style_examples.txt` - Multiple style examples (conservative, aggressive, trend-following, etc.)

#### Step 2: Start Rust MCP Server

```bash
cd rust_mcp_server
cargo run
```

### 📝 Detailed Usage

#### Python Agent Parameters

```bash
uv run mcp_host [OPTIONS]

Options:
  --port PORT              ZMQ service port (required, or set INFER_PORT env var)
  --config PATH            Model config file path (default: model_config.json)
  --trading-style TEXT     Enter trading style text directly
  --trading-style-file PATH Load trading style from file (JSON or text file)
  --prompt TEXT            Test mode: single run and display results (doesn't start server)
  --model-id ID            Model ID (only used in --prompt mode, default: gemini_one)
```

#### Trading Style Definition

Trading style affects all trading decisions of the Agent. You can:

1. **Use Example File**
   ```bash
   uv run mcp_host --port 5001 --trading-style-file trading_style_example.json
   ```

2. **Custom Trading Style File**
   
   Create `my_style.txt`:
   ```
   Conservative Trading Style:
   - Prioritize risk control, single trade risk not exceeding 20% of capital
   - Position management: 30-50% under normal market conditions
   - Focus on stop-loss, set reasonable stop-loss points
   - Based on Z-Score features, be cautious when features deviate significantly (|z| > 2)
   ```
   
   Then run:
   ```bash
   uv run mcp_host --port 5001 --trading-style-file my_style.txt
   ```

3. **Interactive Input**
   ```bash
   uv run mcp_host --port 5001
   # Program will prompt you to enter trading style
   ```

#### Test Mode

Test LLM response (without starting server):
```bash
uv run mcp_host --port 5001 --prompt "How is the current market? What position do you recommend?"
```

### 📊 Example Log Output

After starting, you'll see logs like:

```
[Agent] 🚀 Starting server on port 5001
[Agent] ✅ Loaded 1 model(s)
[Agent] 🔌 ZMQ bound to tcp://127.0.0.1:5001
[Agent] ⏳ Waiting for data from Rust MCP server...
[Agent] 📨 Received request | model_id=gemini_one
[Agent] 📊 Received | price=0.12345 | pos=0.5 | features=10
[Agent] 📝 Generated prompt (1234 chars)
[Agent] 🤖 Calling LLM...
[Agent] 💬 LLM Response: Based on current market data, I recommend...
[Agent] ✅ Decision | cmd=adjust_position | inst=DOGE_USDT_PERP | target_pos=0.6 | latency=3000ms
```

### ⚙️ Configuration

#### Position Weight Range

- **Range**: -1 to 1
- **1.0**: Full long position
- **0.0**: No position (flat)
- **-1.0**: Full short position
- **0.5**: 50% long
- **-0.5**: 50% short

#### Trading Style Types

Refer to `trading_style_examples.txt`, includes:
- **Conservative**: Prioritize risk control, smaller positions
- **Aggressive**: Pursue high returns, can bear larger risks
- **Trend Following**: Follow market trends, go with the flow
- **Mean Reversion**: Reverse operation when price deviates from mean
- **Balanced**: Seek balance between risk and return

### 🔧 Troubleshooting

#### Issue 1: API Key Error
```
ValueError: API key is required
```
**Solution**: Check `api_key` field in `model_config.json`, or set `GEMINI_API_KEY` environment variable

#### Issue 2: Port Already in Use
```
Error: Address already in use
```
**Solution**: Use `--port` parameter to specify another port, or ensure Rust MCP Server uses the same port

#### Issue 3: Timeout Error
```
Model prediction TIMEOUT - skipping this tick
```
**Solution**: 
- Python Agent has been optimized for faster response (reduced token count)
- If still timing out, may need to modify timeout settings in `extrema_infra` library
- Refer to `rust_mcp_server/TIMEOUT_CONFIG.md`

#### Issue 4: Price Data Not Found
```
Price for DOGE_USDT_PERP not available yet
```
**Solution**: Wait a few seconds for WebSocket price data to arrive, this is normal

### 🔌 Supported LLM Providers

- **Gemini**: Google's Gemini models
  - Default model: `gemini-2.5-flash-lite`
  - API Key env var: `GEMINI_API_KEY`
  
- **DeepSeek**: DeepSeek models (OpenAI compatible API)
  - Default model: `deepseek-chat`
  - API Key env var: `DEEPSEEK_API_KEY`
  - Requires: `pip install openai` or `uv sync`

### 📚 More Information

- Python Agent detailed docs: `python_mcp_host/README_USAGE.md`
- Timeout configuration: `rust_mcp_server/TIMEOUT_CONFIG.md`
- Trading style examples: `python_mcp_host/trading_style_examples.txt`
- DeepSeek config example: `python_mcp_host/model_config.deepseek.example.json`

---

## 🎯 Quick Reference

### Common Commands

```bash
# Start Python Agent (interactive style)
cd python_mcp_host
uv run mcp_host --port 5001

# Start Python Agent (with style file)
uv run mcp_host --port 5001 --trading-style-file trading_style_example.json

# Test LLM response
uv run mcp_host --port 5001 --prompt "What position do you recommend?"

# Start Rust Server
cd rust_mcp_server
cargo run
```

### File Structure

```
.
├── python_mcp_host/          # Python LLM Agent
│   ├── src/
│   │   └── python_mcp_host/
│   │       ├── __main__.py   # Main entry point
│   │       ├── infer_host.py # ZMQ server & prompt generation
│   │       └── model_operator.py # LLM & response parsing
│   ├── model_config.json     # Model configuration
│   ├── trading_style_example.json # Trading style example
│   └── README_USAGE.md       # Detailed usage guide
│
└── rust_mcp_server/          # Rust MCP Server
    ├── src/
    │   ├── main.rs           # Main entry point
    │   └── arch/
    │       ├── server_module/ # MCP server logic
    │       └── account_module/ # Account management
    ├── account_config.json   # Account configuration
    └── TIMEOUT_CONFIG.md     # Timeout configuration guide
```

---

## 📞 Support

如有问题，请检查：
1. API Key 是否正确配置
2. 端口号是否一致（默认 5001）
3. 网络连接是否正常
4. 查看日志中的错误信息

For issues, please check:
1. API Key is correctly configured
2. Port numbers match (default 5001)
3. Network connection is normal
4. Check error messages in logs



