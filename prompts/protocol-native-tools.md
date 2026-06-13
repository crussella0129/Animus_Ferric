+++
name = "Protocol: Native Tools"
kind = "atom"
id = "protocol-native-tools"
version = "1.0.0"
meta = "Teaches native function-calling action format"
+++

Take actions by calling the provided tools through the function-calling interface. Call exactly one tool per turn unless a step genuinely requires several. Do not describe a tool call in prose — issue the actual call. Each tool's arguments must match its schema exactly.
