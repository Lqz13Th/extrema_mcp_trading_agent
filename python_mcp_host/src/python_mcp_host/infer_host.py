# infer_server.py
import os
import time
import json
import zmq
import msgpack
import logging
import numpy as np
from .model_operator import LLMLoader, AltTensor

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s | %(levelname)s | %(message)s"
)
logger = logging.getLogger("InferServer")


def alt_tensor_to_prompt(alt_tensor: AltTensor, trading_style: str = None) -> str:
    """
    将 AltTensor 中的信息转换为交易 agent 的 prompt
    用于全自动化交易决策
    
    Rust 发送的数据格式：
    - data: 最新一行的所有特征值（浮点数数组）
    - metadata.col_names: 所有列名（JSON 字符串数组）
    - metadata.price: 当前价格
    - metadata.pos_weight: 当前仓位权重
    """
    metadata = alt_tensor.metadata
    
    # 提取关键信息
    price = metadata.get("price", "未知")
    pos_weight = metadata.get("pos_weight", "0.0")
    col_names_str = metadata.get("col_names", "[]")
    
    # 解析列名
    try:
        col_names = json.loads(col_names_str) if col_names_str else []
    except Exception as e:
        logger.warning(f"[Agent] Failed to parse col_names: {e}")
        col_names = []
    
    # 提取特征数据（转换为可读格式）
    data_values = alt_tensor.data.tolist()
    
    # 验证数据长度匹配
    if len(col_names) != len(data_values):
        logger.warning(
            f"[Agent] Column count mismatch: {len(col_names)} columns but {len(data_values)} values. "
            f"Using indices for unnamed columns."
        )
    
    # 构建 prompt
    prompt_parts = []
    
    # 基础角色设定
    prompt_parts.append("你是一个专业的量化交易员，需要根据实时市场数据做出交易决策。")
    prompt_parts.append("")
    
    # 交易风格定义（如果提供）
    if trading_style:
        prompt_parts.append("## 交易风格")
        prompt_parts.append(trading_style)
        prompt_parts.append("")
    
    # 当前市场信息
    prompt_parts.append("## 当前市场信息")
    prompt_parts.append(f"- 交易对: DOGE_USDT_PERP")
    prompt_parts.append(f"- 当前价格: {price}")
    prompt_parts.append(f"- 当前仓位权重: {pos_weight} (-1到1之间，1表示满仓做多，0表示空仓，-1表示满仓做空)")
    prompt_parts.append("")
    
    # 特征数据 - 分类展示
    if col_names and len(col_names) == len(data_values):
        # 分类特征：原始特征 vs z-score 特征
        raw_features = []
        zscore_features = []
        timestamp_idx = -1
        
        for i, (col_name, value) in enumerate(zip(col_names, data_values)):
            if col_name == "timestamp":
                timestamp_idx = i
                continue
            elif col_name.startswith("z_"):
                # z-score 特征（标准化后的特征，通常在 -3 到 3 之间）
                zscore_features.append((col_name, value))
            else:
                # 原始特征
                raw_features.append((col_name, value))
        
        # 显示原始特征
        if raw_features:
            prompt_parts.append("## 原始市场特征数据")
            for col_name, value in raw_features:
                prompt_parts.append(f"- {col_name}: {value:.6f}")
            prompt_parts.append("")
        
        # 显示 z-score 特征（标准化特征，更易于分析）
        if zscore_features:
            prompt_parts.append("## 标准化特征数据 (Z-Score)")
            prompt_parts.append("(这些特征已经过标准化处理，数值通常在 -3 到 3 之间)")
            prompt_parts.append("(绝对值越大表示偏离均值越远，正值表示高于均值，负值表示低于均值)")
            prompt_parts.append("")
            for col_name, value in zscore_features:
                # 添加解释性标记
                abs_value = abs(value)
                if abs_value > 2.0:
                    significance = "⚠️ 显著偏离"
                elif abs_value > 1.0:
                    significance = "📊 中等偏离"
                else:
                    significance = "✓ 接近均值"
                prompt_parts.append(f"- {col_name}: {value:.4f} {significance}")
            prompt_parts.append("")
        
        # 如果没有分类到任何特征，显示所有特征
        if not raw_features and not zscore_features:
            prompt_parts.append("## 市场特征数据")
            for i, (col_name, value) in enumerate(zip(col_names, data_values)):
                if col_name != "timestamp":
                    prompt_parts.append(f"- {col_name}: {value:.6f}")
            prompt_parts.append("")
    
    elif data_values:
        # 如果没有列名，使用索引
        prompt_parts.append("## 市场特征数据")
        prompt_parts.append(f"- 特征数量: {len(data_values)}")
        for i, value in enumerate(data_values[:20]):  # 显示前20个
            prompt_parts.append(f"  特征[{i}]: {value:.6f}")
        if len(data_values) > 20:
            prompt_parts.append(f"  ... (共 {len(data_values)} 个特征)")
        prompt_parts.append("")
    
    # 任务要求（简化版本，加快 LLM 响应速度）
    prompt_parts.append("## 任务要求")
    prompt_parts.append("请根据以上市场数据做出交易决策。")
    prompt_parts.append("")
    prompt_parts.append("输出格式（必须）：POSITION_SIZE=<数值>")
    prompt_parts.append("- 数值范围：-1到1（1=满仓做多，0=空仓，-1=满仓做空）")
    prompt_parts.append("- 示例：POSITION_SIZE=0.5 或 POSITION_SIZE=-0.3")
    prompt_parts.append("")
    prompt_parts.append("请直接输出 POSITION_SIZE=... 格式：")
    
    return "\n".join(prompt_parts)


def predict_alt_tensor(alt_tensor: AltTensor, model_loader: LLMLoader) -> dict:
    try:
        pred_tensor = model_loader.predict(alt_tensor)
        return pred_tensor.to_dict()
    except Exception as e:
        logger.exception(f"[Error] Model prediction failed: {e}")
        return AltTensor(
            timestamp=int(time.time() * 1000),
            data=np.zeros([1], dtype=np.float32),
            shape=[1],
            metadata={"error": "ERROR_PREDICTION_FAILED", "error_msg": str(e)}
        ).to_dict()


def load_models_for_port(config_path: str, port: int) -> dict:
    if not os.path.exists(config_path):
        raise FileNotFoundError(f"Config file not found: {config_path}")

    with open(config_path, "r") as f:
        config = json.load(f)

    model_map = {}
    for c in config:
        if c.get("port") == port:
            model_id = c["model_id"]
            
            # 只支持LLM模型
            llm_config = {
                "llm_provider": c.get("llm_provider", "gemini"),
                "api_key": c.get("api_key", ""),
                "model_name": c.get("model_name", "gemini-2.0-flash-exp")
            }
            model_map[model_id] = LLMLoader(llm_config)
            logger.info(f"[Init] Loaded LLM model '{model_id}' ({llm_config['llm_provider']}) for port {port}")

    logger.info(f"[Init] Total {len(model_map)} models loaded for port {port}")
    return model_map


def run_server(port: int, config_path: str, trading_style: str = None):
    logger.info(f"[Agent] 🚀 Starting server on port {port}")
    models = load_models_for_port(config_path, port)
    logger.info(f"[Agent] ✅ Loaded {len(models)} model(s)")

    ctx = zmq.Context()
    socket = ctx.socket(zmq.REP)
    socket.bind(f"tcp://127.0.0.1:{port}")
    logger.info(f"[Agent] 🔌 ZMQ bound to tcp://127.0.0.1:{port}")
    logger.info(f"[Agent] ⏳ Waiting for data from Rust MCP server...")

    while True:
        raw = socket.recv()
        try:
            # 接收的数据格式: (timestamp, data_raw, shape, metadata)
            # 严格遵守 Rust AltTensor 定义
            unpacked = msgpack.unpackb(raw, raw=False)
            
            if isinstance(unpacked, dict):
                # 如果是字典格式，转换为元组格式
                timestamp = unpacked.get("timestamp", int(time.time() * 1000))
                data_raw = unpacked.get("data", [])
                shape = unpacked.get("shape", [1])
                metadata = unpacked.get("metadata", {})
            else:
                # 标准格式: (timestamp, data_raw, shape, metadata)
                timestamp, data_raw, shape, metadata = unpacked

            model_id = metadata.get("model_id", "")
            logger.info(f"[Agent] 📨 Received request | model_id={model_id}")

            if model_id not in models:
                logger.error(f"[Agent] ❌ Model '{model_id}' not found on port {port}")
                fallback = AltTensor(
                    timestamp=int(time.time() * 1000),
                    data=np.zeros([1], dtype=np.float32),
                    shape=[1],
                    metadata={"error": "ERROR_MODEL_NOT_FOUND"}
                ).to_dict()
                socket.send(msgpack.packb(fallback, use_bin_type=True))
                continue

            # 将数据转换为numpy数组（必须是浮点数）
            data_np = np.array(data_raw, dtype=np.float32).reshape(shape)
            
            # 验证数据有效性
            if np.any(np.isnan(data_np)) or np.any(np.isinf(data_np)):
                logger.error(f"[Agent] ❌ Invalid input data for model_id={model_id}")
                fallback = AltTensor(
                    timestamp=int(time.time() * 1000),
                    data=np.zeros([1], dtype=np.float32),
                    shape=[1],
                    metadata={"error": "ERROR_INVALID_INPUT"}
                ).to_dict()
                socket.send(msgpack.packb(fallback, use_bin_type=True))
                continue

            # 构造AltTensor（严格遵守Rust定义）
            alt_tensor_input = AltTensor(
                timestamp=timestamp,
                data=data_np,
                shape=list(shape),
                metadata=metadata
            )

            # 显示接收到的关键数据
            price = metadata.get("price", "N/A")
            pos_weight = metadata.get("pos_weight", "0.0")
            data_len = len(data_np)
            logger.info(f"[Agent] 📊 Received | price={price} | pos={pos_weight} | features={data_len}")
            
            # 自动将 AltTensor 信息转换为 prompt（全自动化交易 agent）
            # 如果 metadata 中已经有 prompt，则使用已有的；否则自动生成
            if "prompt" not in metadata or not metadata.get("prompt"):
                auto_prompt = alt_tensor_to_prompt(alt_tensor_input, trading_style=trading_style)
                metadata["prompt"] = auto_prompt
                # 更新 alt_tensor_input 的 metadata
                alt_tensor_input.metadata = metadata
                logger.info(f"[Agent] 📝 Generated prompt ({len(auto_prompt)} chars)")
            
            # LLM预测
            logger.info(f"[Agent] 🤖 Calling LLM...")
            start = time.time()
            result_dict = predict_alt_tensor(alt_tensor_input, models[model_id])
            latency = (time.time() - start) * 1000
            
            # 提取交易决策信息
            result_metadata = result_dict.get("metadata", {})
            response = result_metadata.get("response", "")
            cmd = result_metadata.get("cmd", "noop")
            inst = result_metadata.get("inst", "N/A")
            target_pos = result_metadata.get("target_position", result_metadata.get("pos_weight", "N/A"))
            
            # 显示 LLM 响应摘要
            response_preview = response[:3000] + "..." if len(response) > 3000 else response
            logger.info(f"[Agent] 💬 LLM Response: {response_preview}")
            
            # 显示交易决策
            logger.info(f"[Agent] ✅ Decision | cmd={cmd} | inst={inst} | target_pos={target_pos} | latency={latency:.0f}ms")

            socket.send(msgpack.packb(result_dict, use_bin_type=True))

        except Exception as e:
            logger.error(f"[Agent] ❌ Exception: {e}")
            logger.exception(f"[Agent] Exception details:")
            fallback = AltTensor(
                timestamp=int(time.time() * 1000),
                data=np.zeros([1], dtype=np.float32),
                shape=[1],
                metadata={"error": "ERROR_EXCEPTION", "error_msg": str(e)}
            ).to_dict()
            socket.send(msgpack.packb(fallback, use_bin_type=True))


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=str, default="model_config.json")
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()
    run_server(args.port, args.config)
