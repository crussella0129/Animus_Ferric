import json
import sys

with open(r'C:\Users\charl\.ferric\trace\q-1784174276848.jsonl') as f:
    for line in f:
        data = json.loads(line)
        if 'event' in data:
            e = data['event']
            if e['type'] == 'tool_call':
                print(f"CALL: {e.get('name')} {e.get('args')}")
            elif e['type'] == 'tool_result':
                out = e.get('output', '')
                if len(out) > 100: out = out[:100] + '...'
                print(f"RESULT: {e.get('name')} -> {out}")
            elif e['type'] == 'turn_end':
                text = e.get('text', '')
                try:
                    j = json.loads(text)
                    print(f"THOUGHT: {j.get('thought')}")
                except:
                    pass
