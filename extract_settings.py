import sys

with open('wx_main.py', 'r', encoding='utf-8') as f:
    lines = f.readlines()

start_idx = -1
end_idx = -1

for i, line in enumerate(lines):
    if line.startswith('@dataclass'):
        if i + 1 < len(lines) and lines[i+1].startswith('class Settings:'):
            start_idx = i
            break
            
if start_idx != -1:
    # Find the end of Settings class (first line that has 0 indentation and is not empty, except the @dataclass)
    for i in range(start_idx + 2, len(lines)):
        if lines[i].strip() and not lines[i].startswith(' ') and not lines[i].startswith('\t'):
            end_idx = i
            break

if start_idx != -1 and end_idx != -1:
    settings_code = lines[start_idx:end_idx]
    remaining = lines[:start_idx] + lines[end_idx:]
    
    with open('apricot/settings.py', 'w', encoding='utf-8') as f:
        f.write("from dataclasses import dataclass, field\n")
        f.write("from apricot.constants import *\n\n")
        f.writelines(settings_code)
        
    with open('wx_main.py', 'w', encoding='utf-8') as f:
        # insert import
        remaining.insert(1, "from apricot.settings import Settings\n")
        f.writelines(remaining)
        
    print(f"Extracted {len(settings_code)} lines for Settings.")
else:
    print(f"Failed to find Settings. Start={start_idx}, End={end_idx}")
