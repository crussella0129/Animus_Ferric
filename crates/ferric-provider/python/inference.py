import json
import torch
import traceback
from transformers import AutoModelForCausalLM, AutoTokenizer

_MODEL_CACHE = {}

def load_model(model_dir: str):
    if model_dir in _MODEL_CACHE:
        return _MODEL_CACHE[model_dir]
    
    # We load the tokenizer and model with automatic device placement and half precision
    tokenizer = AutoTokenizer.from_pretrained(model_dir)
    model = AutoModelForCausalLM.from_pretrained(
        model_dir,
        device_map="auto",
        torch_dtype=torch.bfloat16
    )
    
    if not tokenizer.chat_template:
        tokenizer.chat_template = (
            "{% if messages[0]['role'] == 'system' %}"
            "{% set loop_messages = messages[1:] %}"
            "{% set system_message = messages[0]['content'] %}"
            "{% else %}"
            "{% set loop_messages = messages %}"
            "{% set system_message = '' %}"
            "{% endif %}"
            "{% if system_message != '' %}"
            "{{ system_message + '\n' }}"
            "{% endif %}"
            "{% for message in loop_messages %}"
            "{% if message['role'] == 'user' %}"
            "{{ '<start_of_turn>user\n' + message['content'] + '<end_of_turn>\n' }}"
            "{% elif message['role'] == 'model' or message['role'] == 'assistant' %}"
            "{{ '<start_of_turn>model\n' + message['content'] + '<end_of_turn>\n' }}"
            "{% endif %}"
            "{% endfor %}"
            "{% if add_generation_prompt %}"
            "{{ '<start_of_turn>model\n' }}"
            "{% endif %}"
        )
    _MODEL_CACHE[model_dir] = (tokenizer, model)
    return tokenizer, model

def generate_completion(model_dir: str, payload_str: str) -> str:
    try:
        payload = json.loads(payload_str)
        tokenizer, model = load_model(model_dir)
        
        messages = payload.get("messages", [])
        temperature = payload.get("temperature", 0.0)
        max_tokens = payload.get("max_tokens", 512)
        top_p = payload.get("top_p", 1.0)
        
        # Apply the chat template from the model
        inputs = tokenizer.apply_chat_template(
            messages,
            add_generation_prompt=True,
            return_tensors="pt",
            return_dict=True
        ).to(model.device)
        
        # Determine generation kwargs
        do_sample = temperature > 0.0
        gen_kwargs = {
            "max_new_tokens": max_tokens,
            "do_sample": do_sample,
            "pad_token_id": tokenizer.eos_token_id,
        }
        if do_sample:
            gen_kwargs["temperature"] = temperature
            gen_kwargs["top_p"] = top_p
            
        # Generate the output
        outputs = model.generate(**inputs, **gen_kwargs)
        
        # Extract the newly generated tokens
        input_len = inputs["input_ids"].shape[1]
        generated_ids = outputs[0][input_len:]
        
        # Decode the generated text
        text = tokenizer.decode(generated_ids, skip_special_tokens=True)
        
        # Check if it was truncated
        # If the generated length equals max_tokens, it was likely truncated
        truncated = len(generated_ids) >= max_tokens
        
        # Return the payload
        result = {
            "text": text,
            "truncated": truncated,
            "input_tokens": input_len,
            "output_tokens": len(generated_ids),
            "tool_calls": [] # Empty because we disabled native tools in Rust capabilities
        }
        return json.dumps(result)

    except Exception as e:
        traceback.print_exc()
        raise e
