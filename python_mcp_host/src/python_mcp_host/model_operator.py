import os
import json
import logging
import re
import numpy as np
from typing import Dict, List, Optional
import google.generativeai as genai

logger = logging.getLogger("ModelLoader")


# ============================
#  LLM Base Model
# ============================
class BaseLLM:
    def predict_text(self, prompt: str, **kwargs) -> str:
        raise NotImplementedError


# ============================
#  Gemini LLM
# ============================
class GeminiLLM(BaseLLM):
    def __init__(self, api_key: str, model_name: str = "gemini-2.5-flash-lite"):
        if not api_key:
            raise ValueError("Gemini API key is required")
        genai.configure(api_key=api_key)
        self.model = genai.GenerativeModel(model_name)
        self.model_name = model_name
        logger.info(f"[GeminiLLM] Initialized with model: {model_name}")

    def predict_text(self, prompt: str, generation_config: dict = None) -> str:
        try:
            if generation_config:
                response = self.model.generate_content(
                    prompt,
                    generation_config=genai.types.GenerationConfig(**generation_config)
                )
            else:
                response = self.model.generate_content(prompt)
            return response.text
        except Exception as e:
            logger.exception(f"[GeminiLLM] Error generating content: {e}")
            raise


# ============================
#  AltTensor 定义 - 严格遵守Rust结构
# ============================
class AltTensor:
    """
    AltTensor 严格遵守 Rust 定义:
    - timestamp: u64
    - data: Vec<f32> (只能是浮点数向量)
    - shape: Vec<usize>
    - metadata: HashMap<String, String>
    """
    def __init__(self, timestamp: int, data: np.ndarray, shape: List[int], metadata: Dict[str, str]):
        self.timestamp = int(timestamp)
        # data 必须是浮点数数组
        if isinstance(data, (list, tuple)):
            data = np.array(data, dtype=np.float32)
        elif not isinstance(data, np.ndarray):
            raise TypeError(f"data must be numpy array or list, got {type(data)}")
        
        self.data = data.flatten().astype(np.float32)
        self.shape = [int(s) for s in shape]
        self.metadata = {str(k): str(v) for k, v in metadata.items()}  # 确保都是字符串

    def to_dict(self) -> Dict:
        return {
            "timestamp": self.timestamp,
            "data": self.data.tolist(),
            "shape": self.shape,
            "metadata": self.metadata
        }


# ============================
#  LLM 响应解析器 - 提取 Rust Mediator 需要的字段
# ============================
def parse_llm_response_for_mediator(response_text: str) -> Dict[str, str]:
    """
    解析 LLM 响应，提取 Rust MCP Mediator 需要的字段
    支持的命令: adjust_position, risk_alert, query, noop
    
    返回的 metadata 字段:
    - cmd: 命令类型
    - inst: 交易对名称（如 "DOGE_USDT_PERP"）
    - target_position 或 pos_weight: 目标仓位权重（浮点数，转为字符串）
    """
    metadata = {}
    
    # 首先尝试解析 JSON 格式的响应
    json_match = re.search(r'\{[^{}]*"cmd"[^{}]*\}', response_text, re.DOTALL)
    if json_match:
        try:
            json_str = json_match.group(0)
            parsed = json.loads(json_str)
            if isinstance(parsed, dict):
                if "cmd" in parsed:
                    metadata["cmd"] = str(parsed["cmd"])
                if "inst" in parsed:
                    metadata["inst"] = str(parsed["inst"])
                if "target_position" in parsed:
                    metadata["target_position"] = str(parsed["target_position"])
                if "pos_weight" in parsed:
                    metadata["pos_weight"] = str(parsed["pos_weight"])
                logger.info(f"[Parser] Extracted JSON fields: {metadata}")
                return metadata
        except json.JSONDecodeError:
            pass
    
    # 如果没有 JSON，尝试关键词匹配
    response_lower = response_text.lower()
    
    # 检测命令类型
    if "adjust" in response_lower and "position" in response_lower:
        metadata["cmd"] = "adjust_position"
    elif "risk" in response_lower and "alert" in response_lower:
        metadata["cmd"] = "risk_alert"
    elif "query" in response_lower:
        metadata["cmd"] = "query"
    else:
        metadata["cmd"] = "noop"
    
    # 提取交易对名称（常见格式：XXX_USDT_PERP, XXX-USDT-PERP, XXX/USDT）
    inst_patterns = [
        r'([A-Z]+_[A-Z]+_PERP)',  # DOGE_USDT_PERP
        r'([A-Z]+-[A-Z]+-PERP)',   # DOGE-USDT-PERP
        r'([A-Z]+/[A-Z]+)',        # DOGE/USDT
    ]
    for pattern in inst_patterns:
        match = re.search(pattern, response_text, re.IGNORECASE)
        if match:
            inst = match.group(1).upper().replace('-', '_')
            if not inst.endswith('_PERP') and '_' in inst:
                inst = inst + '_PERP'
            metadata["inst"] = inst
            break
    
    # 提取仓位权重（数字，支持负数做空，范围 -1 到 1）
    # 优先匹配 POSITION_SIZE=-0.XX 或 POSITION_SIZE=0.XX 格式（支持负数）
    position_size_match = re.search(r'POSITION_SIZE\s*=\s*(-?[0-9]+\.?[0-9]*)', response_text, re.IGNORECASE)
    if position_size_match:
        weight_str = position_size_match.group(1)
        try:
            weight_val = float(weight_str)
            # 如果是大于1或小于-1的数字，可能是百分比，转换为小数
            if abs(weight_val) > 1.0:
                weight_val = weight_val / 100.0
            # 限制在 -1 到 1 之间（支持做空）
            weight_val = max(-1.0, min(1.0, weight_val))
            metadata["target_position"] = str(weight_val)
            # 如果找到了仓位信息，且 cmd 还是 noop，改为 adjust_position
            if metadata.get("cmd") == "noop":
                metadata["cmd"] = "adjust_position"
        except ValueError:
            pass
    
    # 如果没有找到 POSITION_SIZE，尝试其他格式（支持负数做空）
    if "target_position" not in metadata:
        weight_patterns = [
            r'(?:position|weight|target)[\s:]*(-?[0-9]+\.?[0-9]*)%?',  # 支持负数
            r'(-?[0-9]+\.?[0-9]*)%?\s*(?:position|weight|target)',  # 支持负数
            r'仓位[:\s]*(-?[0-9]+\.?[0-9]*)%?',  # 支持负数
            r'(-?[0-9]+\.?[0-9]*)%?\s*仓位',  # 支持负数
            r'做多[:\s]*([0-9]+\.?[0-9]*)%?',  # 做多
            r'做空[:\s]*([0-9]+\.?[0-9]*)%?',  # 做空（转换为负数）
            # 更宽松的模式：数字后面跟着"仓位"、"做多"、"做空"等关键词
            r'(-?[0-9]+\.?[0-9]+)\s*(?:仓位|做多|做空|position|weight)',  # 确保数字是小数（包含小数点）
        ]
        for i, pattern in enumerate(weight_patterns):
            match = re.search(pattern, response_text, re.IGNORECASE)
            if match:
                weight_str = match.group(1)
                try:
                    weight_val = float(weight_str)
                    
                    # 验证：排除明显不是仓位权重的数字（如年份、大整数等）
                    # 仓位权重通常在 -100 到 100 之间（百分比）或 -1 到 1 之间（小数）
                    # 如果数字绝对值大于 1000，很可能是年份或其他无关数字，跳过
                    if abs(weight_val) > 1000:
                        logger.debug(f"[Parser] Skipping unlikely weight value: {weight_val} (too large)")
                        continue
                    
                    # 如果是"做空"模式，转换为负数
                    if i == len(weight_patterns) - 1:  # 最后一个模式是"做空"
                        weight_val = -abs(weight_val)
                    
                    # 如果是大于1或小于-1的数字，可能是百分比，转换为小数
                    if abs(weight_val) > 1.0:
                        weight_val = weight_val / 100.0
                    
                    # 限制在 -1 到 1 之间（支持做空）
                    weight_val = max(-1.0, min(1.0, weight_val))
                    
                    # 最终验证：如果转换后的值接近0且原始值很大，可能是误匹配
                    if abs(weight_val) < 0.01 and abs(float(weight_str)) > 10:
                        logger.debug(f"[Parser] Skipping unlikely weight value: {weight_str} -> {weight_val} (suspicious conversion)")
                        continue
                    
                    metadata["target_position"] = str(weight_val)
                    # 如果找到了仓位信息，且 cmd 还是 noop，改为 adjust_position
                    if metadata.get("cmd") == "noop":
                        metadata["cmd"] = "adjust_position"
                    break
                except ValueError:
                    continue
    
    # 如果没有找到交易对，使用默认值（Rust mediator 需要 inst 字段）
    # 只要 cmd 是 adjust_position，就需要 inst 字段
    if "inst" not in metadata and metadata.get("cmd") == "adjust_position":
        metadata["inst"] = "DOGE_USDT_PERP"  # Rust mediator 的默认值
        logger.info("[Parser] No instrument found in response, using default: DOGE_USDT_PERP")
    
    if metadata:
        logger.info(f"[Agent] 🔍 Parsed | {', '.join([f'{k}={v}' for k, v in metadata.items()])}")
    
    return metadata


# ============================
#  LLM Loader
# ============================
class LLMLoader:
    def __init__(self, config: Dict):
        """
        从配置加载LLM模型
        config应包含: api_key, model_name, llm_provider等
        """
        self.config = config
        self.llm_provider = config.get("llm_provider", "gemini").lower()
        self.api_key = config.get("api_key", "")
        self.model_name = config.get("model_name", "gemini-2.5-flash-lite")
        
        if not self.api_key:
            # 尝试从环境变量获取
            self.api_key = os.getenv("GEMINI_API_KEY", "")
        
        if not self.api_key:
            raise ValueError("API key is required. Set it in config or GEMINI_API_KEY environment variable")
        
        self.model = self._load_model()
        logger.info(f"[LLMLoader] Loaded {self.llm_provider} model: {self.model_name}")

    def _load_model(self) -> BaseLLM:
        if self.llm_provider == "gemini":
            return GeminiLLM(api_key=self.api_key, model_name=self.model_name)
        else:
            raise ValueError(f"Unsupported LLM provider: {self.llm_provider}")

    def predict(self, alt_tensor: AltTensor) -> AltTensor:
        """
        LLM文本预测
        文本通过 metadata["prompt"] 传递
        输出文本通过 metadata["response"] 传递，data 包含编码后的响应
        """
        # 从metadata获取prompt
        prompt = alt_tensor.metadata.get("prompt", "")
        if not prompt:
            # 如果没有prompt，尝试从data构造（将浮点数解码为文本）
            # 这里可以添加编码/解码逻辑，但通常prompt应该在metadata中
            logger.warning("[LLMLoader] No prompt in metadata, using empty prompt")
            prompt = ""
        
        # 从metadata获取额外的生成参数
        temperature = float(alt_tensor.metadata.get("temperature", "0.7"))
        max_tokens = int(alt_tensor.metadata.get("max_tokens", "1000"))
        
        # 调用LLM
        try:
            response_text = self.model.predict_text(
                prompt,
                generation_config={
                    "temperature": temperature,
                    "max_output_tokens": max_tokens,
                }
            )
        except Exception as e:
            logger.exception(f"[LLMLoader] LLM prediction failed: {e}")
            response_text = f"ERROR: {str(e)}"
        
        # 将响应文本编码为浮点数数组（ASCII编码）
        # 每个字符转换为ASCII码，然后归一化到0-1范围
        response_bytes = response_text.encode('utf-8')
        response_data = np.array([b / 255.0 for b in response_bytes], dtype=np.float32)
        
        # 解析 LLM 响应，提取 Rust Mediator 需要的字段
        mediator_fields = parse_llm_response_for_mediator(response_text)
        
        # 构造返回AltTensor
        metadata = alt_tensor.metadata.copy()
        metadata["model_type"] = f"{self.llm_provider}_{self.model_name}"
        metadata["response"] = response_text  # 完整响应保存在metadata中
        metadata["prompt"] = prompt  # 保存原始prompt
        
        # 添加 Rust Mediator 需要的字段
        # 如果 LLM 响应中没有提取到 cmd，默认使用 "noop"
        if "cmd" not in mediator_fields:
            mediator_fields["cmd"] = "noop"
        
        # 合并 mediator 字段到 metadata（确保都是字符串）
        for key, value in mediator_fields.items():
            metadata[key] = str(value)
        
        logger.debug(f"[LLMLoader] Final metadata keys: {list(metadata.keys())}")
        
        return AltTensor(
            timestamp=alt_tensor.timestamp,
            data=response_data,
            shape=[len(response_data)],
            metadata=metadata
        )
