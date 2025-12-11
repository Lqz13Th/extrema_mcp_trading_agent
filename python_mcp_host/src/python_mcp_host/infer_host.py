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


def run_server(port: int, config_path: str):
    logger.info(f"[Start] Infer Server on port {port}")
    models = load_models_for_port(config_path, port)

    ctx = zmq.Context()
    socket = ctx.socket(zmq.REP)
    socket.bind(f"tcp://127.0.0.1:{port}")
    logger.info(f"[ZMQ] Bound tcp://127.0.0.1:{port}")

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

            if model_id not in models:
                logger.error(f"[Error] model_id={model_id} not found on port {port}")
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
                logger.error(f"[Error] Invalid input for model_id={model_id}")
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

            # LLM预测
            start = time.time()
            result_dict = predict_alt_tensor(alt_tensor_input, models[model_id])
            latency = (time.time() - start) * 1000
            
            # 记录日志
            prompt = metadata.get("prompt", "")[:100]  # 截取前100字符
            response = result_dict.get("metadata", {}).get("response", "")
            response_len = len(response) if response else 0
            
            logger.info(
                "[Infer] model_id=%s, prompt_len=%d, response_len=%d, latency=%.2fms",
                model_id,
                len(prompt),
                response_len,
                latency,
            )

            socket.send(msgpack.packb(result_dict, use_bin_type=True))

        except Exception as e:
            logger.exception(f"[Exception] Failed to handle request: {e}")
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
