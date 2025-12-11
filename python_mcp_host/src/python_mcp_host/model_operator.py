import os
import json
import logging
import numpy as np
from typing import Dict, List
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
    def __init__(self, api_key: str, model_name: str = "gemini-2.0-flash-exp"):
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
        self.model_name = config.get("model_name", "gemini-2.0-flash-exp")
        
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
        
        # 构造返回AltTensor
        metadata = alt_tensor.metadata.copy()
        metadata["model_type"] = f"{self.llm_provider}_{self.model_name}"
        metadata["response"] = response_text  # 完整响应保存在metadata中
        metadata["prompt"] = prompt  # 保存原始prompt
        
        return AltTensor(
            timestamp=alt_tensor.timestamp,
            data=response_data,
            shape=[len(response_data)],
            metadata=metadata
        )
