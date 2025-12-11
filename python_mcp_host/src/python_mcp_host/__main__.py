# extrema_infer/__main__.py
import os
import sys
import argparse
import time
import json
import logging
import numpy as np
from .infer_host import run_server, load_models_for_port
from .model_operator import AltTensor

logger = logging.getLogger("Main")

def _parse_args(argv=None):
    parser = argparse.ArgumentParser(prog="extrema_infer")
    parser.add_argument(
        "--port",
        type=int,
        required=False,
        help="ZMQ service port (can also use INFER_PORT env var)"
    )
    parser.add_argument(
        "--config",
        type=str,
        default="model_config.json",
        help="模型配置文件路径"
    )
    parser.add_argument(
        "--prompt",
        type=str,
        required=False,
        help="从命令行输入 prompt，如果不提供则启动 ZMQ 服务器模式"
    )
    parser.add_argument(
        "--model-id",
        type=str,
        default="gemini_one",
        help="模型 ID（仅在 --prompt 模式下使用）"
    )
    parser.add_argument(
        "--trading-style",
        type=str,
        required=False,
        help="交易风格定义（可以是文件路径或直接输入文本）。如果不提供，将交互式询问"
    )
    parser.add_argument(
        "--trading-style-file",
        type=str,
        required=False,
        help="交易风格文件路径（JSON 或文本文件）"
    )
    return parser.parse_args(argv)

def run_prompt_mode(port: int, config_path: str, prompt: str, model_id: str):
    """测试模式：从命令行输入 prompt，调用 LLM 并返回结果（仅用于测试）"""
    import logging
    logging.basicConfig(level=logging.INFO)
    logger = logging.getLogger("PromptMode")
    
    # 加载模型
    models = load_models_for_port(config_path, port)
    if model_id not in models:
        raise SystemExit(f"Model ID '{model_id}' not found on port {port}")
    
    model_loader = models[model_id]
    
    # 构造 AltTensor 输入
    timestamp = int(time.time() * 1000)
    metadata = {
        "model_id": model_id,
        "prompt": prompt,
    }
    
    alt_tensor_input = AltTensor(
        timestamp=timestamp,
        data=np.zeros([1], dtype=np.float32),  # 空数据，prompt 在 metadata 中
        shape=[1],
        metadata=metadata
    )
    
    # 调用 LLM 预测
    logger.info(f"[Prompt] Sending prompt to model '{model_id}'...")
    logger.info(f"[Prompt] Prompt: {prompt[:100]}...")
    
    try:
        result_tensor = model_loader.predict(alt_tensor_input)
        result_dict = result_tensor.to_dict()
        
        # 打印结果
        response = result_dict.get("metadata", {}).get("response", "")
        print("\n" + "="*60)
        print("LLM Response:")
        print("="*60)
        print(response)
        print("="*60)
        
        # 打印解析后的 metadata（Rust mediator 需要的字段）
        print("\nMetadata for Rust MCP Mediator:")
        print("-"*60)
        mediator_metadata = {}
        for key in ["cmd", "inst", "target_position", "pos_weight"]:
            if key in result_dict.get("metadata", {}):
                value = result_dict["metadata"][key]
                mediator_metadata[key] = value
                print(f"  {key}: {value}")
        
        if not mediator_metadata:
            print("  (No mediator fields found in response)")
        
        print("-"*60)
        print(f"\nFull metadata: {json.dumps(result_dict.get('metadata', {}), indent=2, ensure_ascii=False)}")
        
    except Exception as e:
        logger.exception(f"[Error] LLM prediction failed: {e}")
        raise SystemExit(1)

def load_trading_style(style_input: str = None, style_file: str = None) -> str:
    """
    加载交易风格定义
    优先级：style_file > style_input > 交互式输入
    """
    # 如果提供了文件路径，从文件读取
    if style_file and os.path.exists(style_file):
        try:
            with open(style_file, 'r', encoding='utf-8') as f:
                content = f.read().strip()
                if style_file.endswith('.json'):
                    # 如果是 JSON 文件，尝试解析
                    data = json.loads(content)
                    if isinstance(data, dict) and 'trading_style' in data:
                        return data['trading_style']
                    elif isinstance(data, str):
                        return data
                return content
        except Exception as e:
            logger.warning(f"[Agent] Failed to load trading style from file {style_file}: {e}")
    
    # 如果直接提供了文本
    if style_input:
        return style_input.strip()
    
    # 交互式输入
    print("\n" + "="*60)
    print("交易 Agent 风格定义")
    print("="*60)
    print("请定义你的交易风格，这将影响 agent 的所有交易决策。")
    print("例如：")
    print("  - 稳健型：优先控制风险，仓位较小，注重止损")
    print("  - 激进型：追求高收益，可以承担较大风险")
    print("  - 趋势跟踪：跟随市场趋势，顺势而为")
    print("  - 均值回归：在价格偏离均值时反向操作")
    print("  - 或者自定义你的交易策略和风险偏好")
    print("="*60)
    
    trading_style = input("\n请输入交易风格（可以直接回车使用默认稳健风格）: ").strip()
    
    if not trading_style:
        trading_style = """稳健型交易风格：
- 优先控制风险，单次交易风险不超过总资金的 20%
- 仓位管理：正常市场条件下仓位控制在 30-50%，极端市场条件下降低到 10-20%
- 注重止损，设置合理的止损点位
- 不追求短期暴利，注重长期稳定收益
- 在市场不确定性高时，倾向于减少仓位或空仓
- 基于 Z-Score 特征，当特征显著偏离（|z| > 2）时，谨慎操作"""
        print(f"\n使用默认稳健风格:\n{trading_style}\n")
    else:
        print(f"\n已设置交易风格:\n{trading_style}\n")
    
    return trading_style

def main(argv=None):
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s | %(levelname)s | %(message)s"
    )
    
    args = _parse_args(argv)
    port = args.port or int(os.environ.get("INFER_PORT", 0) or 0)
    if not port:
        raise SystemExit("port is required: use --port 5001 or set INFER_PORT env var")
    
    # 如果提供了 --prompt，进入测试模式（单次运行）
    if args.prompt:
        run_prompt_mode(port, args.config, args.prompt, args.model_id)
    else:
        # 加载交易风格
        trading_style = load_trading_style(args.trading_style, args.trading_style_file)
        
        # 启动 ZMQ 服务器（全自动化交易 agent，持续运行）
        logger.info("[Agent] Starting automated trading agent (ZMQ server mode)...")
        logger.info("[Agent] Trading style loaded successfully")
        logger.info("[Agent] Waiting for Rust MCP server to send AltTensor data...")
        run_server(port, config_path=args.config, trading_style=trading_style)

def run(port=None, trading_style=None):
    if port is None:
        args = _parse_args(sys.argv[1:])
        port = args.port or int(os.environ.get("INFER_PORT", 0) or 0)
        if not port:
            print("ERROR: port is required. Use `--port 5001` or set INFER_PORT env var.")
            return
        config_path = args.config
        trading_style = load_trading_style(args.trading_style, args.trading_style_file)
    else:
        config_path = "model_config.json"
        if trading_style is None:
            trading_style = load_trading_style()
    run_server(port, config_path=config_path, trading_style=trading_style)

if __name__ == "__main__":
    main()

# uv run mcp_host --port 5001
